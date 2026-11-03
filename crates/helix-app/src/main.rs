//! Iron Helix.
//!
//! So far: the interface laid out from the original score, scaled to fit
//! any screen with decorative side panels, and navigation through the whole
//! ship (helix-core): movies, location strips, doors, ladders, elevators,
//! the map and sounds, plus the PC release's hidden rooms when its files
//! are given.
//!
//! Desktop: `ironhelix [--mac-app <Iron Helix™.rsrc>] [--mac-files <Iron Helix™ Files>]
//! [--mac-sounds <Iron Helix Sounds>] [--pc-files <HELIX>] [--side-panel <png>] [--window WxH]
//! [--screenshot <png>]`.
//! Keys: arrows move/turn, U/D ladders, Tab repeat moves, Enter door, 1-6
//! elevator, M map mode, P game phase (testing), F pixel-perfect scaling,
//! H debug overlay, Esc quit.

mod assets;
mod layout;
mod patches;
mod screen;
mod sound;
mod sprites;
mod video;

use assets::{GameAssets, Paths};
use layout::Layout;
use macroquad::prelude::*;
use screen::GameScreen;
use sprites::Canvas;

const PANEL_COLOR: Color = Color::new(0.10, 0.10, 0.11, 1.0);

/// Keys passed on to the game screen.
const GAME_KEYS: [KeyCode; 19] = [
    KeyCode::Up,
    KeyCode::Down,
    KeyCode::Left,
    KeyCode::Right,
    KeyCode::U,
    KeyCode::D,
    KeyCode::Tab,
    KeyCode::Enter,
    KeyCode::KpEnter,
    KeyCode::Key1,
    KeyCode::Key2,
    KeyCode::Key3,
    KeyCode::Key4,
    KeyCode::Key5,
    KeyCode::Key6,
    KeyCode::Key7,
    KeyCode::Space,
    KeyCode::M,
    KeyCode::P,
];

fn window_conf() -> Conf {
    // `--window 2400x1080` tries other screen shapes on the desktop.
    let args: Vec<String> = std::env::args().collect();
    let (window_width, window_height) = args
        .windows(2)
        .find(|p| p[0] == "--window")
        .and_then(|p| {
            let (w, h) = p[1].split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or((1280, 720));
    Conf {
        window_title: "Iron Helix".to_owned(),
        window_width,
        window_height,
        high_dpi: true,
        window_resizable: true,
        ..Default::default()
    }
}

/// Draw the decorative artwork beside (or above/below) the stage. The art
/// is anchored to the edge touching the stage and cropped on the outer
/// edge; the right panel is the left one mirrored.
fn draw_panels(l: &Layout, art: Option<&Texture2D>) {
    for (r, mirrored) in [(l.left, false), (l.right, true)] {
        if r.w < 1.0 {
            continue;
        }
        match art {
            Some(tex) => {
                let tex_scale = r.h / tex.height();
                let visible = (r.w / tex_scale).min(tex.width());
                let source = macroquad::math::Rect::new(tex.width() - visible, 0.0, visible, tex.height());
                draw_texture_ex(
                    tex,
                    r.x,
                    r.y,
                    WHITE,
                    DrawTextureParams {
                        dest_size: Some(vec2(r.w, r.h)),
                        source: Some(source),
                        flip_x: mirrored,
                        ..Default::default()
                    },
                );
            }
            None => draw_rectangle(r.x, r.y, r.w, r.h, PANEL_COLOR),
        }
    }
    for r in [l.top, l.bottom] {
        if r.h >= 1.0 {
            draw_rectangle(r.x, r.y, r.w, r.h, PANEL_COLOR);
        }
    }
}

async fn show_error(message: String) {
    eprintln!("{message}");
    loop {
        clear_background(BLACK);
        draw_text("Iron Helix couldn't start:", 20.0, 40.0, 28.0, WHITE);
        for (i, line) in message.lines().enumerate() {
            draw_text(line, 20.0, 80.0 + i as f32 * 26.0, 22.0, LIGHTGRAY);
        }
        draw_text("Press Esc to quit.", 20.0, screen_height() - 30.0, 22.0, GRAY);
        if is_key_pressed(KeyCode::Escape) {
            return;
        }
        next_frame().await;
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    let paths = Paths::from_args();
    let setup = GameAssets::load(&paths).and_then(|a| GameScreen::new(&a).map(|s| (a, s)));
    let (assets, mut game) = match setup {
        Ok(ok) => ok,
        Err(e) => {
            show_error(format!("{e}\n\nPoint --mac-app and --mac-files at your extracted Mac game files.")).await;
            return;
        }
    };

    let side_art = std::fs::read(&paths.side_panel)
        .ok()
        .map(|bytes| Texture2D::from_file_with_format(&bytes, None));

    let mut stage = Image {
        width: assets.stage_w as u16,
        height: assets.stage_h as u16,
        bytes: vec![0; assets.stage_w * assets.stage_h * 4],
    };
    let stage_tex = Texture2D::from_image(&stage);
    stage_tex.set_filter(FilterMode::Nearest);

    let mut pixel_perfect = false;
    let mut show_debug = std::env::args().any(|a| a == "--debug");
    let mut frames_drawn = 0u32;
    // `--autoplay FFLRO2`: moves fed in one at a time (F/B forward/back,
    // L/R turn, U/D ladder, O door, 1-6 elevator, M map mode, P phase), for testing.
    let args: Vec<String> = std::env::args().collect();
    let mut autoplay: std::collections::VecDeque<KeyCode> = args
        .windows(2)
        .find(|p| p[0] == "--autoplay")
        .map(|p| {
            p[1].chars()
                .filter_map(|c| match c.to_ascii_uppercase() {
                    'F' => Some(KeyCode::Up),
                    'B' => Some(KeyCode::Down),
                    'L' => Some(KeyCode::Left),
                    'R' => Some(KeyCode::Right),
                    'U' => Some(KeyCode::U),
                    'D' => Some(KeyCode::D),
                    'O' => Some(KeyCode::Enter),
                    'M' => Some(KeyCode::M),
                    'P' => Some(KeyCode::P),
                    '1' => Some(KeyCode::Key1),
                    '2' => Some(KeyCode::Key2),
                    '3' => Some(KeyCode::Key3),
                    '4' => Some(KeyCode::Key4),
                    '5' => Some(KeyCode::Key5),
                    '6' => Some(KeyCode::Key6),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let autoplaying = !autoplay.is_empty();
    let mut idle_frames = 0u32;

    loop {
        if is_key_pressed(KeyCode::Escape) {
            break;
        }
        if is_key_pressed(KeyCode::F) {
            pixel_perfect = !pixel_perfect;
        }
        if is_key_pressed(KeyCode::H) {
            show_debug = !show_debug;
        }
        for key in GAME_KEYS {
            if is_key_pressed(key) {
                game.key(key);
            }
        }
        if !game.busy() {
            if let Some(key) = autoplay.pop_front() {
                game.key(key);
            }
            idle_frames += 1;
        } else {
            idle_frames = 0;
        }

        let layout = Layout::fit(screen_width(), screen_height(), assets.stage_w as f32, assets.stage_h as f32, pixel_perfect);

        // Mouse and touch both become taps in stage coordinates.
        let mut taps = Vec::new();
        if is_mouse_button_pressed(MouseButton::Left) {
            let (x, y) = mouse_position();
            taps.extend(layout.screen_to_stage(x, y));
        }
        for t in touches() {
            if t.phase == TouchPhase::Started {
                taps.extend(layout.screen_to_stage(t.position.x, t.position.y));
            }
        }
        for (x, y) in taps {
            game.tap(x, y);
        }

        game.update(&assets, get_frame_time()).await;

        game.draw(&mut Canvas { pixels: &mut stage.bytes, width: assets.stage_w, height: assets.stage_h });
        stage_tex.update(&stage);

        clear_background(BLACK);
        draw_panels(&layout, side_art.as_ref());
        draw_texture_ex(
            &stage_tex,
            layout.stage.x,
            layout.stage.y,
            WHITE,
            DrawTextureParams { dest_size: Some(vec2(layout.stage.w, layout.stage.h)), ..Default::default() },
        );

        if show_debug {
            let nav = game.navigator();
            let lines = [
                format!("node {} (line {}) facing {} {}  phase {}", nav.node().id, nav.line(), game.facing(), if nav.in_ladder() { "in ladder" } else { "" }, nav.phase()),
                format!("PC rooms: {}", if assets.pc_rooms.is_empty() { "none (no PC files)".to_owned() } else { assets.pc_rooms.join(", ") }),
                format!("last movie {}", game.status),
                format!("stage {}x{} at {:.2}x{}", assets.stage_w, assets.stage_h, layout.scale, if pixel_perfect { " (pixel perfect)" } else { "" }),
                "arrows move/turn  U/D ladders  Tab repeat  Enter door  1-6 elevator  M map  P phase  F pixel-perfect  H hide  Esc quit".to_owned(),
            ];
            for (i, line) in lines.iter().enumerate() {
                draw_text(line, 8.0, 18.0 + i as f32 * 18.0, 18.0, YELLOW);
            }
        }

        frames_drawn += 1;
        let done = if autoplaying { autoplay.is_empty() && idle_frames > 20 } else { frames_drawn == 150 };
        if let Some(path) = &paths.screenshot
            && done
        {
            get_screen_data().export_png(&path.to_string_lossy());
            break;
        }

        next_frame().await;
    }
}
