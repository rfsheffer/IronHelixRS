//! Iron Helix.
//!
//! The whole game: the opening and title screen, then play (helix-core)
//! with the original interface laid out from the Director score, scaled to
//! fit any screen with decorative side panels. The PC release's hidden
//! rooms and the Sega CD's theme and extended ending join in when their
//! files are given.
//!
//! Desktop: `ironhelix [--mac-app <Iron Helix™.rsrc>] [--mac-files <Iron Helix™ Files>]
//! [--mac-sounds <Iron Helix Sounds>] [--pc-files <HELIX>] [--sega-cue <cue>]
//! [--sega-files <folder>] [--save <file>] [--side-panel <png>] [--window WxH]
//! [--skill 1-5] [--seed N] [--skip-title] [--screenshot <png>]`.
//! Keys: arrows move/turn, U/D ladders, Tab repeat moves, Enter door, 1-6
//! elevator, S scan, C probe controls, N notes, P pause, Esc quit dialog,
//! M map mode; F2 game phase and G grant DNA and skip to phase 2 (testing);
//! F pixel-perfect scaling, H debug overlay.

mod assets;
mod ending;
mod icon;
mod layout;
mod patches;
mod screen;
mod sound;
mod sprites;
mod title;
mod video;

use assets::{GameAssets, Paths};
use layout::Layout;
use macroquad::prelude::*;
use ending::Ending;
use screen::{GameScreen, Start};
use sprites::Canvas;
use title::{Choice, TitleScreen};

/// Beside (or above and below) the stage when there is no side art.
const PANEL_COLOR: Color = BLACK;

/// Keys passed on to the game screen.
const GAME_KEYS: [KeyCode; 31] = [
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
    KeyCode::S,
    KeyCode::C,
    KeyCode::G,
    KeyCode::X,
    KeyCode::Key0,
    KeyCode::Key8,
    KeyCode::Key9,
    KeyCode::Backspace,
    KeyCode::N,
    KeyCode::Escape,
    KeyCode::Q,
    KeyCode::F2,
];

/// Keys the title screen takes.
const TITLE_KEYS: [KeyCode; 14] = [
    KeyCode::Key1,
    KeyCode::Key2,
    KeyCode::Key3,
    KeyCode::Key4,
    KeyCode::Key5,
    KeyCode::Enter,
    KeyCode::KpEnter,
    KeyCode::B,
    KeyCode::L,
    KeyCode::R,
    KeyCode::A,
    KeyCode::Q,
    KeyCode::Escape,
    KeyCode::Space,
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
        // The Mac release's probe icon, from the player's files.
        icon: icon::mac_icon(&Paths::from_args().mac_app),
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

/// The notes over the view window, wrapped to its width, in the original's
/// green with a shadow so they read over the picture.
fn draw_notes(l: &Layout, lines: &[String]) {
    let s = l.scale;
    let size = (11.0 * s).round() as u16;
    let line_height = 12.0 * s;
    let (x0, top) = (l.stage.x + screen::NOTES_AT.0 * s, l.stage.y + screen::NOTES_AT.1 * s);
    let width = screen::NOTES_WIDTH * s;
    let bottom = l.stage.y + screen::NOTES_BOTTOM * s;
    let mut y = top + line_height;
    let green = Color::from_rgba(0x00, 0xFF, 0x33, 0xFF);
    for line in lines {
        // Word-wrap to the view's width.
        let mut current = String::new();
        let mut rows = Vec::new();
        for word in line.split(' ') {
            let candidate = if current.is_empty() { word.to_string() } else { format!("{current} {word}") };
            if measure_text(&candidate, None, size, 1.0).width > width && !current.is_empty() {
                rows.push(std::mem::replace(&mut current, word.to_string()));
            } else {
                current = candidate;
            }
        }
        rows.push(current);
        for row in rows {
            if y > bottom {
                return;
            }
            draw_text(&row, x0 + s, y + s, size as f32, BLACK);
            draw_text(&row, x0, y, size as f32, green);
            y += line_height;
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

/// One `--autoplay` step.
enum Step {
    Key(KeyCode),
    Click(f32, f32),
}

/// `FFLRO2(380,60)A`: letters are keys (F/B forward/back, L/R turn, U/D
/// ladder, O door, 1-6 elevator, M map mode, P phase (F2), S scan, C controls,
/// X arm, K click the scan point (or the port with the arm out), A acquire,
/// G grant DNA and skip to phase 2, Y be caught by the defender, J jam, N notes, digits on the
/// keypad); `(x,y)` is a click.
fn parse_autoplay(s: &str) -> std::collections::VecDeque<Step> {
    let mut steps = std::collections::VecDeque::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '(' {
            let inner: String = chars.by_ref().take_while(|&c| c != ')').collect();
            if let Some((x, y)) = inner.split_once(',')
                && let (Ok(x), Ok(y)) = (x.trim().parse(), y.trim().parse())
            {
                steps.push_back(Step::Click(x, y));
            }
            continue;
        }
        let key = match c.to_ascii_uppercase() {
            'F' => KeyCode::Up,
            'B' => KeyCode::Down,
            'L' => KeyCode::Left,
            'R' => KeyCode::Right,
            'U' => KeyCode::U,
            'D' => KeyCode::D,
            'O' => KeyCode::Enter,
            'M' => KeyCode::M,
            'P' => KeyCode::F2,
            'S' => KeyCode::S,
            'C' => KeyCode::C,
            'K' => KeyCode::K,
            'A' => KeyCode::A,
            'G' => KeyCode::G,
            'Y' => KeyCode::Y,
            'J' => KeyCode::Space,
            'N' => KeyCode::N,
            'X' => KeyCode::X,
            '0' => KeyCode::Key0,
            '7' => KeyCode::Key7,
            '8' => KeyCode::Key8,
            '9' => KeyCode::Key9,
            '1' => KeyCode::Key1,
            '2' => KeyCode::Key2,
            '3' => KeyCode::Key3,
            '4' => KeyCode::Key4,
            '5' => KeyCode::Key5,
            '6' => KeyCode::Key6,
            _ => continue,
        };
        steps.push_back(Step::Key(key));
    }
    steps
}

/// What's on screen.
enum Mode {
    Title,
    Game(Box<GameScreen>),
    Ending(Box<Ending>),
}

/// A press, drag or release at stage coordinates.
enum Input {
    Press(f32, f32),
    Drag(f32, f32),
    Release,
}

#[macroquad::main(window_conf)]
async fn main() {
    let paths = Paths::from_args();
    let setup = GameAssets::load(&paths).and_then(|a| TitleScreen::new(&a).map(|t| (a, t)));
    let (assets, mut title) = match setup {
        Ok(ok) => ok,
        Err(e) => {
            show_error(format!("{e}\n\nPoint --mac-app and --mac-files at your extracted Mac game files.")).await;
            return;
        }
    };
    // Each new game gets its own seed; the first is `--seed`'s.
    let mut seed = assets.seed;
    // `--play-ending` (testing): the Sega CD's extended ending at once.
    let play_ending = std::env::args().any(|a| a == "--play-ending");
    let mut mode = if let Some(ending) = assets.ending.as_deref().filter(|_| play_ending).and_then(|p| Ending::load(p).ok()) {
        Mode::Ending(Box::new(ending))
    } else if assets.skip_title {
        match GameScreen::new(&assets, assets.skill, seed, Start::Plain) {
            Ok(game) => Mode::Game(Box::new(game)),
            Err(e) => {
                show_error(e).await;
                return;
            }
        }
    } else {
        title.intro(&assets).await;
        Mode::Title
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
    // `--autoplay FFLRO2(380,60)`: steps fed in one at a time, for testing
    // (see parse_autoplay).
    let args: Vec<String> = std::env::args().collect();
    let mut autoplay: std::collections::VecDeque<Step> = args
        .windows(2)
        .find(|p| p[0] == "--autoplay")
        .map(|p| parse_autoplay(&p[1]))
        .unwrap_or_default();
    let autoplaying = !autoplay.is_empty();
    let shot_frame: Option<u32> = args.windows(2).find(|p| p[0] == "--screenshot-frame").and_then(|p| p[1].parse().ok());
    let mut idle_frames = 0u32;

    loop {
        if is_key_pressed(KeyCode::F) {
            pixel_perfect = !pixel_perfect;
        }
        if is_key_pressed(KeyCode::H) {
            show_debug = !show_debug;
        }
        let layout = Layout::fit(screen_width(), screen_height(), assets.stage_w as f32, assets.stage_h as f32, pixel_perfect);

        // Mouse and touch both become presses, drags and releases in stage
        // coordinates.
        let mut inputs = Vec::new();
        let (mx, my) = mouse_position();
        if is_mouse_button_pressed(MouseButton::Left) {
            if let Some((x, y)) = layout.screen_to_stage(mx, my) {
                inputs.push(Input::Press(x, y));
            }
        } else if is_mouse_button_down(MouseButton::Left)
            && let Some((x, y)) = layout.screen_to_stage(mx, my)
        {
            inputs.push(Input::Drag(x, y));
        }
        if is_mouse_button_released(MouseButton::Left) {
            inputs.push(Input::Release);
        }
        for t in touches() {
            let at = layout.screen_to_stage(t.position.x, t.position.y);
            match (t.phase, at) {
                (TouchPhase::Started, Some((x, y))) => inputs.push(Input::Press(x, y)),
                (TouchPhase::Moved, Some((x, y))) => inputs.push(Input::Drag(x, y)),
                (TouchPhase::Ended | TouchPhase::Cancelled, _) => inputs.push(Input::Release),
                _ => {}
            }
        }
        let dt = get_frame_time();
        let mut canvas = Canvas { pixels: &mut stage.bytes, width: assets.stage_w, height: assets.stage_h };

        let mut next = None;
        match &mut mode {
            Mode::Title => {
                for key in TITLE_KEYS {
                    if is_key_pressed(key) {
                        title.key(key);
                    }
                }
                for input in &inputs {
                    if let Input::Press(x, y) = input {
                        title.press(*x, *y);
                    }
                }
                // Autoplay with `--title`: a step every half second.
                if frames_drawn % 30 == 29 {
                    match autoplay.pop_front() {
                        Some(Step::Key(key)) => title.key(key),
                        Some(Step::Click(x, y)) => title.press(x, y),
                        None => {}
                    }
                }
                title.update(&assets, dt).await;
                match title.take_choice() {
                    Some(Choice::Begin(skill)) => {
                        seed = seed.wrapping_add(1);
                        match GameScreen::new(&assets, skill, seed, Start::Intro) {
                            Ok(game) => next = Some(Mode::Game(Box::new(game))),
                            Err(e) => eprintln!("{e}"),
                        }
                    }
                    Some(Choice::Load(text)) => {
                        seed = seed.wrapping_add(1);
                        match GameScreen::new(&assets, assets.skill, seed, Start::Load(text)) {
                            Ok(game) => next = Some(Mode::Game(Box::new(game))),
                            Err(e) => {
                                eprintln!("{e}");
                                title.sounds().play("SysBeeb").await;
                                title.menu(&assets).await;
                            }
                        }
                    }
                    Some(Choice::Quit) => break,
                    None => {}
                }
                title.draw(&mut canvas);
            }
            Mode::Game(game) => {
                for key in GAME_KEYS {
                    if is_key_pressed(key) {
                        game.key(key);
                    }
                }
                if !game.busy() {
                    match autoplay.pop_front() {
                        Some(Step::Key(key)) => game.key(key),
                        Some(Step::Click(x, y)) => {
                            game.press(x, y);
                            game.release();
                        }
                        None => {}
                    }
                    idle_frames += 1;
                } else {
                    idle_frames = 0;
                }
                for input in &inputs {
                    match *input {
                        Input::Press(x, y) => game.press(x, y),
                        Input::Drag(x, y) => game.drag(x, y),
                        Input::Release => game.release(),
                    }
                }
                game.update(&assets, dt).await;
                if game.quitting() {
                    title.leave(&assets).await;
                    next = Some(Mode::Title);
                } else if game.finished() {
                    // A skill-5 win goes on to the Sega CD's ending, then the
                    // title (Lingo's NPReturn).
                    let ending = assets.ending.as_deref().filter(|_| game.extended_ending()).and_then(|p| match Ending::load(p) {
                        Ok(e) => Some(e),
                        Err(e) => {
                            eprintln!("{e}");
                            None
                        }
                    });
                    match ending {
                        Some(e) => next = Some(Mode::Ending(Box::new(e))),
                        None => {
                            title.menu(&assets).await;
                            next = Some(Mode::Title);
                        }
                    }
                }
                game.draw(&mut canvas);
            }
            Mode::Ending(ending) => {
                let skip = is_key_pressed(KeyCode::Escape) || inputs.iter().any(|i| matches!(i, Input::Press(..)));
                if ending.update(dt, title.sounds()).await || skip {
                    title.menu(&assets).await;
                    next = Some(Mode::Title);
                }
                ending.draw(&mut canvas);
            }
        }
        if let Some(m) = next {
            mode = m;
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

        if let Mode::Game(game) = &mode {
            // The notes, over the view (9-point green text in the original).
            if let Some(lines) = game.notes() {
                draw_notes(&layout, &lines);
            }
            if let Some(text) = game.toast() {
                draw_notes(&layout, &[text.to_string()]);
            }
            if show_debug {
                draw_debug(&layout, &assets, game, pixel_perfect);
            }
        }

        frames_drawn += 1;
        // `--screenshot-frame N` shoots at that frame instead (mid-movie).
        let done = match shot_frame {
            Some(n) => frames_drawn == n,
            None if autoplaying => autoplay.is_empty() && idle_frames > 20,
            None => frames_drawn == 150,
        };
        if let Some(path) = &paths.screenshot
            && done
        {
            get_screen_data().export_png(&path.to_string_lossy());
            break;
        }

        next_frame().await;
    }
}

/// The debug overlay (H): the game's state and the keys.
fn draw_debug(layout: &Layout, assets: &GameAssets, game: &GameScreen, pixel_perfect: bool) {
    let nav = game.navigator();
    let g = game.game();
    let lines = [
        format!(
            "skill {}  energy {}/{}  DNA kit {:?}  auto scan {}  {:.1} min left{}",
            g.skill(),
            g.energy(),
            helix_core::game::ENERGY_FULL,
            g.kit(),
            if g.auto_scan() { "on" } else { "off" },
            g.minutes_left(),
            if g.probe_dead() { "  PROBE DOWN (reboot)" } else { "" }
        ),
        format!("node {} (line {}) facing {} {}  phase {}", nav.node().id, nav.line(), game.facing(), if nav.in_ladder() { "in ladder" } else { "" }, nav.phase()),
        format!(
            "probe {}  defender {} warning {}{}{}  plan {:?}",
            g.probe_number(),
            g.defender().id(),
            g.defender().warning(),
            if g.defender().off() { " off" } else { "" },
            if g.defender().jammed() { " jammed" } else { "" },
            g.defender().plan()
        ),
        format!(
            "codes {} {:?}  clues {:?} seen {:?}  defender tasks {:?}",
            g.codes().0,
            g.codes().1,
            g.clues().0,
            g.clues().1,
            g.defender_tasks()
        ),
        format!("PC rooms: {}", if assets.pc_rooms.is_empty() { "none (no PC files)".to_owned() } else { assets.pc_rooms.join(", ") }),
        format!("last movie {}", game.status),
        format!("stage {}x{} at {:.2}x{}", assets.stage_w, assets.stage_h, layout.scale, if pixel_perfect { " (pixel perfect)" } else { "" }),
        "arrows move/turn  U/D ladders  Tab repeat  Enter door  1-6 elevator  S scan  Space jam  X arm  C controls  N notes  P pause  M map  F2 phase  G phase 2  F pixel-perfect  H hide  Esc quit".to_owned(),
    ];
    for (i, line) in lines.iter().enumerate() {
        draw_text(line, 8.0, 18.0 + i as f32 * 18.0, 18.0, YELLOW);
    }
}
