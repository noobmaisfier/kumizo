//! Sonde : rockbox-playback lit-il nos 3 formats (opus/stéréo, mp3, m4a/3GP) ?
//! Lecture réelle séquentielle (UN SEUL test : les codecs Rockbox partagent
//! un `codec_api` global, une seule session de décodage à la fois).
//! Volume à 5 % : on vérifie que la position avance, pas les oreilles.
//! `cargo test -p aurora --test rockbox_probe -- --nocapture`

use std::time::{Duration, Instant};

use rockbox_playback::{OutputConfig, PlaybackState, PlayerConfig};

const FILES: [&str; 3] = [
    "/home/alamine/Music/Seydi-Mouhamed-El-Cheikh-Lite/01_Audio/01_Conferences-et-Dahiras/KORITE 2017.opus",
    "/home/alamine/Music/Les-Dimensions-10-K7/Les Dimensions de CHEIKHOU TIDIANE n8.mp3",
    "/home/alamine/Music/Prophetes-et-Recits/An Nisâ(Les Femmes).m4a",
];

/// Joue `path`, attend le démarrage (≤ 10 s), mesure l'avance sur `secs`.
fn play_for(path: &str, secs: u64) -> Result<(f64, f64, f64), String> {
    let player = PlayerConfig::builder()
        .output(OutputConfig::Cpal)
        .open()
        .map_err(|e| format!("player: {e:?}"))?;
    player.set_volume(0.05);
    player.set_queue(vec![path.to_string()]);
    player.play();
    let t0 = Instant::now();
    loop {
        if player.status().state == PlaybackState::Playing {
            break;
        }
        if t0.elapsed() > Duration::from_secs(10) {
            return Err(format!("jamais joué : {path}"));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let st = player.status();
    let (p1, dur) = (
        st.position.as_secs_f64(),
        st.duration.as_secs_f64(),
    );
    std::thread::sleep(Duration::from_secs(secs));
    let st = player.status();
    if st.state != PlaybackState::Playing {
        return Err(format!("arrêté trop tôt : {path}"));
    }
    Ok((p1, st.position.as_secs_f64(), dur))
}

#[test]
fn rockbox_formats() {
    for f in FILES {
        let (a, b, dur) = play_for(f, 2).unwrap_or_else(|e| panic!("{e}"));
        println!("{f}\n  position : {a:.1}s -> {b:.1}s (durée annoncée {dur:.0}s)");
        assert!(b > a, "position figée : {f}");
    }
}
