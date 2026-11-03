//! Iron Helix.
//!
//! For now: the interface, scaled to fit any screen with decorative side
//! panels, playing navigation movies in the view window. Game rules come
//! next (see docs/design.md).
//!
//! Desktop: `ironhelix [--mac-app <Iron Helix™.rsrc>] [--mac-files <Iron Helix™ Files>]
//! [--side-panel <png>] [--screenshot <png>]`. Keys: N next movie, F toggle pixel-perfect scaling,
//! H toggle the debug overlay, Esc quit.

mod assets;
mod layout;
mod video;

use std::path::PathBuf;

use assets::{GameAssets, Paths};
use layout::Layout;
use macroquad::prelude::*;
use video::{Clip, Player};

/// Where the 240x128 navigation movies sit on the stage. Their inner
/// 233x123 (offset 4, 3) exactly fills the white view window of the
/// interface artwork at (25, 19).
const VIEW_ORIGIN: (usize, usize) = (21, 16);
const VIEW_INNER: (usize, usize, usize, usize) = (4, 3, 233, 123);

/// The game's starting node; its movies make a handy demo.
const DEMO_NODE: (&str, &str) = ("L4", "4.04.");

const PANEL_COLOR: Color = Color::new(0.10, 0.10, 0.11, 1.0);

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

/// Copy `rect` (x, y, w, h) of an RGBA frame onto the stage at (dx, dy).
fn blit(stage: &mut Image, frame: &[u8], frame_w: usize, rect: (usize, usize, usize, usize), dx: usize, dy: usize) {
    let (sx, sy, w, h) = rect;
    let stage_w = stage.width as usize;
    for row in 0..h {
        let src = ((sy + row) * frame_w + sx) * 4;
        let dst = ((dy + row) * stage_w + dx) * 4;
        if let (Some(s), true) = (frame.get(src..src + w * 4), dst + w * 4 <= stage.bytes.len()) {
            stage.bytes[dst..dst + w * 4].copy_from_slice(s);
        }
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

fn demo_movies(assets: &GameAssets) -> Vec<PathBuf> {
    let dir = assets.file(&["Navigation", DEMO_NODE.0]);
    let mut movies: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    name.starts_with(DEMO_NODE.1) && !name.ends_with(".rsrc")
                })
                .collect()
        })
        .unwrap_or_default();
    movies.sort();
    movies
}

#[macroquad::main(window_conf)]
async fn main() {
    let paths = Paths::from_args();
    let assets = match GameAssets::load(&paths) {
        Ok(a) => a,
        Err(e) => {
            show_error(format!("{e}\n\nPoint --mac-app and --mac-files at your extracted Mac game files.")).await;
            return;
        }
    };

    let side_art = std::fs::read(&paths.side_panel)
        .ok()
        .and_then(|bytes| Texture2D::from_file_with_format(&bytes, None).into());

    let mut stage = Image {
        width: assets.stage_w as u16,
        height: assets.stage_h as u16,
        bytes: assets.interface.clone(),
    };
    let stage_tex = Texture2D::from_image(&stage);
    stage_tex.set_filter(FilterMode::Nearest);

    let movies = demo_movies(&assets);
    let mut movie_index = 0;
    let mut player: Option<Player> = None;
    let mut pixel_perfect = false;
    let mut show_debug = true;
    let mut last_tap: Option<(f32, f32)> = None;
    let mut status = String::new();
    let mut frames_drawn = 0u32;

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
        let next_requested = is_key_pressed(KeyCode::N);

        let layout = Layout::fit(screen_width(), screen_height(), assets.stage_w as f32, assets.stage_h as f32, pixel_perfect);

        // Mouse and touch both become stage coordinates.
        if is_mouse_button_pressed(MouseButton::Left) {
            let (x, y) = mouse_position();
            last_tap = layout.screen_to_stage(x, y);
        }
        for t in touches() {
            if t.phase == TouchPhase::Started {
                last_tap = layout.screen_to_stage(t.position.x, t.position.y);
            }
        }

        // Movie playback: advance, and move on when a clip ends.
        let finished = player.as_mut().is_none_or(|p| p.update(get_frame_time()));
        if (finished || next_requested) && !movies.is_empty() {
            if player.is_some() {
                movie_index = (movie_index + 1) % movies.len();
            }
            let path = &movies[movie_index];
            match Clip::load(path) {
                Ok(clip) => {
                    status = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    player = Some(Player::new(clip));
                }
                Err(e) => {
                    status = e;
                    player = None;
                }
            }
        }

        // Compose the stage: interface, then the movie in the view window.
        stage.bytes.copy_from_slice(&assets.interface);
        if let Some(p) = &player
            && let Some(frame) = p.current()
        {
            let (ix, iy, _, _) = VIEW_INNER;
            blit(&mut stage, frame, p.clip.width, VIEW_INNER, VIEW_ORIGIN.0 + ix, VIEW_ORIGIN.1 + iy);
        }
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
            let tap = last_tap.map_or("-".to_owned(), |(x, y)| format!("{x:.0}, {y:.0}"));
            let lines = [
                format!("movie {status}"),
                format!("stage {}x{} at {:.2}x{}", assets.stage_w, assets.stage_h, layout.scale, if pixel_perfect { " (pixel perfect)" } else { "" }),
                format!("last tap (stage coords): {tap}"),
                "N next movie  F pixel-perfect  H hide  Esc quit".to_owned(),
            ];
            for (i, line) in lines.iter().enumerate() {
                draw_text(line, 8.0, 18.0 + i as f32 * 18.0, 18.0, YELLOW);
            }
        }

        frames_drawn += 1;
        if let Some(path) = &paths.screenshot
            && frames_drawn == 150
        {
            get_screen_data().export_png(&path.to_string_lossy());
            break;
        }

        next_frame().await;
    }
}
