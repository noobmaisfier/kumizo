//! Minimal Linux-PAM password check.
//!
//! Five functions and a conversation callback, hand-written FFI against
//! `libpam.so` (present everywhere PAM is). The `pam` crate was deliberately
//! not used: it builds through `bindgen`, which needs `libclang` on every
//! contributor's machine, for an API surface this small.
//!
//! The only entry point is [`verify`]: synchronous and blocking (PAM may
//! converse with `unix_chkpwd`), so call it off the UI thread. It wipes its
//! copy of the secret before returning.

use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};

// PAM return codes, from <security/pam_appl.h>. Named to keep the mapping in
// [`classify`] readable and auditable.
const SUCCESS: c_int = 0;
const BUF_ERR: c_int = 5;
const PERM_DENIED: c_int = 6;
const AUTH_ERR: c_int = 7;
const CRED_INSUFFICIENT: c_int = 8;
const AUTHINFO_UNAVAIL: c_int = 9;
const USER_UNKNOWN: c_int = 10;
const MAXTRIES: c_int = 11;
const CRED_EXPIRED: c_int = 16;
const CRED_ERR: c_int = 17;
const CONV_ERR: c_int = 19;

const PROMPT_ECHO_OFF: c_int = 2;

/// Outcome of [`verify`].
///
/// Deliberately fine-grained: the *log* needs to know a faillock lockout from
/// a backend outage from a plain typo. The *UI* must not: telling a stranger
/// "unknown user" turns the login prompt into a user-enumeration oracle. Use
/// [`AuthError::user_message`], which is vague on purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthError {
    /// The password was wrong. PAM does not distinguish "wrong" from "hash
    /// unreadable" — both surface as `PAM_AUTH_ERR`.
    WrongPassword,
    /// The account exists in the auth backend but is not a local account, or
    /// the backend does not know it (AD/LDAP domain user, …).
    UnknownUser,
    /// `pam_faillock` (or similar) counted too many failures and is now
    /// refusing the account outright, regardless of the password.
    LockedOut,
    /// The authentication backend could not be reached at all (nsswitch down,
    /// directory socket gone). Not a credential problem: retrying may work.
    Unavailable,
    /// Anything else, with the raw PAM status for the log line.
    System(c_int),
}

impl AuthError {
    /// Message safe to display to whoever is sitting at the lock screen.
    ///
    /// Only states the user can act on survive; everything else is folded
    /// into a generic failure so the prompt leaks nothing about the account.
    pub fn user_message(self) -> &'static str {
        match self {
            AuthError::WrongPassword | AuthError::UnknownUser => "Wrong password",
            AuthError::LockedOut => "Too many attempts — try again shortly",
            AuthError::Unavailable => "Authentication service unavailable",
            AuthError::System(_) => "Login failed",
        }
    }
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::WrongPassword => write!(f, "Wrong password"),
            AuthError::UnknownUser => write!(f, "Account unknown to the auth backend"),
            AuthError::LockedOut => write!(f, "Account locked out after too many failures"),
            AuthError::Unavailable => write!(f, "Authentication backend unavailable"),
            AuthError::System(code) => write!(f, "Login failed (PAM status {code})"),
        }
    }
}

/// Map a raw PAM status to a typed outcome.
///
/// Split out from [`verify`] so the mapping is testable without ever calling
/// into PAM: tests of this function cannot trip `pam_faillock`.
fn classify(status: c_int) -> Option<AuthError> {
    match status {
        SUCCESS => None,
        // The password itself was rejected.
        AUTH_ERR => Some(AuthError::WrongPassword),
        USER_UNKNOWN => Some(AuthError::UnknownUser),
        MAXTRIES => Some(AuthError::LockedOut),
        // The backend could not answer — a directory socket that is down, a
        // name service timeout. Distinct from a rejected credential.
        AUTHINFO_UNAVAIL => Some(AuthError::Unavailable),
        // Insufficient credentials/privileges of the calling application,
        // permission denied, or invalid/expired credentials.
        CRED_INSUFFICIENT | PERM_DENIED | CRED_ERR | CRED_EXPIRED => {
            Some(AuthError::System(status))
        }
        other => Some(AuthError::System(other)),
    }
}

#[repr(C)]
struct Handle {
    _opaque: [u8; 0],
}

#[repr(C)]
struct Message {
    style: c_int,
    #[allow(dead_code)]
    text: *const c_char,
}

#[repr(C)]
struct Response {
    value: *mut c_char,
    #[allow(dead_code)]
    code: c_int,
}

#[repr(C)]
struct Conv {
    /// `*mut *const Message` is layout-identical to C's
    /// `const struct pam_message **`: the outer pointer is mutable, the
    /// messages it points at are not. No `*const` cast is possible here
    /// because we are the callee and PAM owns that buffer.
    func: Option<
        extern "C" fn(c_int, *mut *const Message, *mut *mut Response, *mut c_void) -> c_int,
    >,
    appdata: *mut c_void,
}

/// Credentials handed to the conversation through `appdata`. Lives on the
/// caller's stack for the whole synchronous `pam_authenticate` call.
struct Creds {
    password: CString,
}

/// Free `responses` plus the first `allocated` answer strings.
///
/// Per `pam_conv(3)`: on success PAM takes ownership of the array *and* of
/// each `resp` string and frees them itself. On an error return PAM frees
/// nothing, so the whole partial allocation is ours to release.
unsafe fn cleanup_responses(responses: *mut Response, allocated: usize) {
    for j in 0..allocated {
        let value = (*responses.add(j)).value;
        if !value.is_null() {
            libc::free(value as *mut c_void);
        }
    }
    libc::free(responses as *mut c_void);
}

extern "C" fn converse(
    count: c_int,
    messages: *mut *const Message,
    out: *mut *mut Response,
    appdata: *mut c_void,
) -> c_int {
    if out.is_null() {
        return CONV_ERR;
    }
    // Never hand PAM a non-null `*resp` on the error paths: it would then own
    // a pointer we are about to free.
    unsafe { *out = std::ptr::null_mut() };

    if count <= 0 || messages.is_null() {
        return CONV_ERR;
    }
    let creds = unsafe { &*(appdata as *const Creds) };
    let n = count as usize;

    // Memory lifecycle per Linux-PAM pam_conv(3):
    // "It is the caller's responsibility to release both, this array and the
    // responses themselves, using free(3)." On failure, converse must clean
    // up its own partial allocations.
    // We use libc::calloc and libc::strdup to ensure PAM can safely free(3) them.
    // PAM reads every slot, and a zeroed slot is the correct "no answer" for
    // informational messages.
    let responses =
        unsafe { libc::calloc(n, std::mem::size_of::<Response>()) as *mut Response };
    if responses.is_null() {
        return BUF_ERR;
    }

    for i in 0..n {
        let message = unsafe { *messages.add(i) };
        if message.is_null() {
            unsafe { cleanup_responses(responses, i) };
            return CONV_ERR;
        }

        // Only a masked prompt gets the secret. Answering a PROMPT_ECHO_ON
        // question ("login:", OTP confirmation) with the password would
        // hand it to a caller that is explicitly allowed to display it.
        // Everything else is left null, which is the documented way to say
        // "no answer" and keeps `motd`-style info messages harmless.
        if unsafe { (*message).style } == PROMPT_ECHO_OFF {
            // PAM frees the answer with `free()`, so `strdup` is exactly the
            // right allocator.
            let dup = unsafe { libc::strdup(creds.password.as_ptr()) };
            if dup.is_null() {
                // Free the strings already duplicated in this loop, not just
                // the array: a bare `free(responses)` would leak them.
                unsafe { cleanup_responses(responses, i) };
                return BUF_ERR;
            }
            unsafe {
                (*responses.add(i)).value = dup;
            }
        }
    }

    // Ownership transfers to PAM: it frees the array and every string in it.
    unsafe { *out = responses };
    SUCCESS
}

#[link(name = "pam")]
extern "C" {
    fn pam_start(
        service: *const c_char,
        user: *const c_char,
        conv: *const Conv,
        handle: *mut *mut Handle,
    ) -> c_int;
    fn pam_authenticate(handle: *mut Handle, flags: c_int) -> c_int;
    fn pam_acct_mgmt(handle: *mut Handle, flags: c_int) -> c_int;
    fn pam_end(handle: *mut Handle, status: c_int) -> c_int;
}

/// Overwrite a buffer with zeroes in a way the optimiser may not remove.
///
/// A plain `for b in buf { *b = 0 }` writes to memory that is never read
/// again before being freed, which is exactly the shape LLVM's dead-store
/// elimination targets — the wipe can be compiled away entirely. Volatile
/// writes are ordered side effects and survive.
fn wipe(buf: &mut [u8]) {
    for b in buf {
        unsafe { std::ptr::write_volatile(b, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

/// Check `password` for `user` against `service` (e.g. `"login"`).
///
/// Synchronous and blocking — call off the UI thread.
pub fn verify(service: &str, user: &str, password: &str) -> Result<(), AuthError> {
    let svc = CString::new(service).map_err(|_| AuthError::System(-1))?;
    let login = CString::new(user).map_err(|_| AuthError::System(-2))?;
    // An interior NUL would silently truncate the secret: refuse instead.
    let secret = CString::new(password).map_err(|_| AuthError::WrongPassword)?;
    let creds = Creds { password: secret };
    let conv = Conv {
        func: Some(converse),
        appdata: &creds as *const Creds as *mut c_void,
    };

    let mut handle: *mut Handle = std::ptr::null_mut();
    let started = unsafe { pam_start(svc.as_ptr(), login.as_ptr(), &conv, &mut handle) };
    let status = if started != SUCCESS || handle.is_null() {
        started
    } else {
        let auth = unsafe { pam_authenticate(handle, 0) };
        if auth == SUCCESS {
            unsafe { pam_acct_mgmt(handle, 0) }
        } else {
            auth
        }
    };
    if !handle.is_null() {
        unsafe {
            pam_end(handle, status);
        }
    }
    // Wipe the last copy of the secret before it leaves the frame. Note the
    // `strdup` copies handed to PAM are freed (and only freed) by PAM itself.
    let mut raw = creds.password.into_bytes_with_nul();
    wipe(&mut raw);
    drop(raw);

    match classify(status) {
        None => Ok(()),
        Some(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // These tests never call PAM: a real `pam_authenticate` failure would feed
    // `pam_faillock` and slowly lock the developer's own account.

    #[test]
    fn status_mapping() {
        assert_eq!(classify(SUCCESS), None);
        assert_eq!(classify(AUTH_ERR), Some(AuthError::WrongPassword));
        assert_eq!(
            classify(CRED_INSUFFICIENT),
            Some(AuthError::System(CRED_INSUFFICIENT))
        );
        assert_eq!(classify(USER_UNKNOWN), Some(AuthError::UnknownUser));
        assert_eq!(classify(MAXTRIES), Some(AuthError::LockedOut));
        assert_eq!(classify(AUTHINFO_UNAVAIL), Some(AuthError::Unavailable));
        assert_eq!(classify(PERM_DENIED), Some(AuthError::System(PERM_DENIED)));
        assert_eq!(classify(CRED_ERR), Some(AuthError::System(CRED_ERR)));
        assert_eq!(classify(CRED_EXPIRED), Some(AuthError::System(CRED_EXPIRED)));
        assert_eq!(classify(4242), Some(AuthError::System(4242)));
    }

    #[test]
    fn user_message_never_confirms_an_account_exists() {
        // A user-enumeration guard: the two "this account is not usable"
        // states must be indistinguishable on screen.
        assert_eq!(
            AuthError::UnknownUser.user_message(),
            AuthError::WrongPassword.user_message()
        );
        // Internal failures must not leak a PAM code either.
        assert!(!AuthError::System(4242).user_message().contains("4242"));
    }

    #[test]
    fn nul_byte_is_refused() {
        // Rejects before any PAM call, so this is safe to run anywhere.
        assert_eq!(
            verify("login", "someone", "a\0b"),
            Err(AuthError::WrongPassword)
        );
    }

    #[test]
    fn wipe_actually_zeroes() {
        let mut buf = *b"secret bytes";
        wipe(&mut buf);
        assert_eq!(buf, [0u8; 12]);
    }
}