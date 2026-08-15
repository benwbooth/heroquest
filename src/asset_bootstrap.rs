use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow};
use fontdue::Font;
use image::{Rgba, RgbaImage};
use sdl3::{
    EventPump, Sdl, VideoSubsystem, event::Event, keyboard::Keycode, mouse::MouseButton,
    pixels::PixelFormat, render::WindowCanvas, surface::Surface,
};

const WIDTH: u32 = 960;
const HEIGHT: u32 = 640;
const SCAN_ARCHIVE_BYTES: u64 = 1_902_474_536;
const MODEL_SOURCE_BYTES: u64 = 705_614_733;
const ACCEPT_BUTTON: UiRect = UiRect::new(494, 520, 346, 62);
const QUIT_BUTTON: UiRect = UiRect::new(120, 520, 250, 62);
const CANCEL_BUTTON: UiRect = UiRect::new(690, 520, 150, 54);
const RETRY_BUTTON: UiRect = UiRect::new(494, 520, 346, 62);

pub enum AssetBootstrapOutcome {
    Ready,
    Quit,
}

enum InstallAttempt {
    Complete,
    Cancelled,
    Failed(String),
}

#[derive(Clone, Copy)]
struct UiRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl UiRect {
    const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x as f32
            && x <= (self.x + self.width) as f32
            && y >= self.y as f32
            && y <= (self.y + self.height) as f32
    }
}

#[derive(Clone)]
struct ProgressState {
    amount: f32,
    stage: String,
    detail: String,
}

impl Default for ProgressState {
    fn default() -> Self {
        Self {
            amount: 0.01,
            stage: "Starting the complete asset installer".to_owned(),
            detail: "The game will remain responsive while assets are prepared.".to_owned(),
        }
    }
}

pub fn ensure_complete_assets(sdl: &Sdl, video: &VideoSubsystem) -> Result<AssetBootstrapOutcome> {
    if complete_runtime_assets_are_installed() {
        return Ok(AssetBootstrapOutcome::Ready);
    }

    let window = video
        .window("HeroQuest - First Run Setup", WIDTH, HEIGHT)
        .position_centered()
        .build()
        .map_err(|error| anyhow!(error.to_string()))?;
    let mut canvas = window.into_canvas();
    let mut events = sdl.event_pump()?;
    let font = Font::from_bytes(
        include_bytes!("../assets/fonts/Almendra-Regular.ttf") as &[u8],
        fontdue::FontSettings::default(),
    )
    .map_err(|error| anyhow!("failed to load bundled setup font: {error}"))?;

    loop {
        if !wait_for_consent(&mut canvas, &mut events, &font)? {
            return Ok(AssetBootstrapOutcome::Quit);
        }
        match run_installer(&mut canvas, &mut events, &font)? {
            InstallAttempt::Complete => return Ok(AssetBootstrapOutcome::Ready),
            InstallAttempt::Cancelled => return Ok(AssetBootstrapOutcome::Quit),
            InstallAttempt::Failed(error) => {
                if !wait_for_retry(&mut canvas, &mut events, &font, &error)? {
                    return Ok(AssetBootstrapOutcome::Quit);
                }
            }
        }
    }
}

fn original_us_art_root() -> PathBuf {
    std::env::var_os("HEROQUEST_ART_DIR").map_or_else(
        || PathBuf::from("assets/local/editions/original-us"),
        PathBuf::from,
    )
}

fn model_source_root() -> PathBuf {
    std::env::var_os("HEROQUEST_MODEL_SOURCE_DIR").map_or_else(
        || PathBuf::from("assets/local/sources/greengreenwine-community-pack"),
        PathBuf::from,
    )
}

fn asset_installer_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("HEROQUEST_ASSET_INSTALLER").map(PathBuf::from) {
        return path.is_file().then_some(path);
    }
    let source_tree = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tools/install-all-assets.sh");
    if source_tree.is_file() {
        return Some(source_tree);
    }
    std::env::current_exe()
        .ok()
        .and_then(|executable| executable.parent().map(Path::to_path_buf))
        .map(|directory| directory.join("tools/install-all-assets.sh"))
        .filter(|path| path.is_file())
}

fn complete_runtime_assets_are_installed() -> bool {
    let Some(installer) = asset_installer_path() else {
        return false;
    };
    Command::new("bash")
        .arg(installer)
        .arg("--check")
        .env("HEROQUEST_ART_DIR", original_us_art_root())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn wait_for_consent(
    canvas: &mut WindowCanvas,
    events: &mut EventPump,
    font: &Font,
) -> Result<bool> {
    render_consent(canvas, font)?;
    loop {
        for event in events.poll_iter() {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => return Ok(false),
                Event::KeyDown {
                    keycode: Some(Keycode::Return),
                    repeat: false,
                    ..
                } => return Ok(true),
                Event::MouseButtonDown {
                    mouse_btn: MouseButton::Left,
                    x,
                    y,
                    ..
                } if ACCEPT_BUTTON.contains(x, y) => return Ok(true),
                Event::MouseButtonDown {
                    mouse_btn: MouseButton::Left,
                    x,
                    y,
                    ..
                } if QUIT_BUTTON.contains(x, y) => return Ok(false),
                _ => {}
            }
        }
        thread::sleep(Duration::from_millis(16));
    }
}

fn run_installer(
    canvas: &mut WindowCanvas,
    events: &mut EventPump,
    font: &Font,
) -> Result<InstallAttempt> {
    let Some(installer) = asset_installer_path() else {
        return Ok(InstallAttempt::Failed(
            "The complete installer is not available beside this build. Set HEROQUEST_ASSET_INSTALLER to its path."
                .to_owned(),
        ));
    };
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let progress_path = std::env::temp_dir().join(format!(
        "heroquest-install-progress-{}-{nonce}.txt",
        std::process::id()
    ));
    let log_path = std::env::temp_dir().join(format!(
        "heroquest-install-{}-{nonce}.log",
        std::process::id()
    ));
    File::create(&progress_path)
        .with_context(|| format!("failed to create {}", progress_path.display()))?;
    let log = File::create(&log_path)
        .with_context(|| format!("failed to create {}", log_path.display()))?;
    let log_error = log.try_clone()?;
    let mut child = Command::new("bash")
        .arg(&installer)
        .arg("--accept-liability")
        .env("HEROQUEST_ART_DIR", original_us_art_root())
        .env("HEROQUEST_PROGRESS_FILE", &progress_path)
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_error))
        .spawn()
        .with_context(|| format!("failed to launch asset installer {}", installer.display()))?;

    let mut state = ProgressState::default();
    let mut last_render = Instant::now() - Duration::from_secs(1);
    loop {
        for event in events.poll_iter() {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => {
                    stop_child(&mut child);
                    let _ = fs::remove_file(&progress_path);
                    return Ok(InstallAttempt::Cancelled);
                }
                Event::MouseButtonDown {
                    mouse_btn: MouseButton::Left,
                    x,
                    y,
                    ..
                } if CANCEL_BUTTON.contains(x, y) => {
                    stop_child(&mut child);
                    let _ = fs::remove_file(&progress_path);
                    return Ok(InstallAttempt::Cancelled);
                }
                _ => {}
            }
        }

        if let Some(parsed) = read_progress(&progress_path) {
            state = measured_progress(parsed);
        }
        if last_render.elapsed() >= Duration::from_millis(80) {
            render_progress(canvas, font, &state)?;
            last_render = Instant::now();
        }

        if let Some(status) = child.try_wait()? {
            let _ = fs::remove_file(&progress_path);
            if status.success() && complete_runtime_assets_are_installed() {
                state = ProgressState {
                    amount: 1.0,
                    stage: "All HeroQuest assets are ready".to_owned(),
                    detail: "Starting the game...".to_owned(),
                };
                render_progress(canvas, font, &state)?;
                thread::sleep(Duration::from_millis(450));
                let _ = fs::remove_file(&log_path);
                return Ok(InstallAttempt::Complete);
            }
            return Ok(InstallAttempt::Failed(installer_failure(status, &log_path)));
        }
        thread::sleep(Duration::from_millis(16));
    }
}

fn stop_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn read_progress(path: &Path) -> Option<ProgressState> {
    let text = fs::read_to_string(path).ok()?;
    text.lines().rev().find_map(|line| {
        let (amount, stage) = line.split_once('\t')?;
        Some(ProgressState {
            amount: amount.parse::<f32>().ok()?.clamp(0.0, 1.0),
            stage: stage.trim().to_owned(),
            detail: "Please leave HeroQuest open while this step completes.".to_owned(),
        })
    })
}

fn measured_progress(mut state: ProgressState) -> ProgressState {
    if state.stage == "Downloading original-US scan archive" {
        let source = original_us_art_root().join("source");
        let partial = source.join("HQ Game System US.rar.part");
        let archive = source.join("HQ Game System US.rar");
        let bytes = fs::metadata(&partial)
            .or_else(|_| fs::metadata(&archive))
            .map_or(0, |metadata| metadata.len());
        let ratio = bytes as f32 / SCAN_ARCHIVE_BYTES as f32;
        state.amount = 0.02 + ratio.clamp(0.0, 1.0) * 0.55;
        state.detail = format!(
            "Original-US scans: {:.1} / {:.1} MiB",
            bytes as f64 / 1_048_576.0,
            SCAN_ARCHIVE_BYTES as f64 / 1_048_576.0
        );
    } else if state.stage == "Downloading classic STL source collection" {
        let bytes = directory_size(&model_source_root());
        let ratio = bytes as f32 / MODEL_SOURCE_BYTES as f32;
        state.amount = 0.73 + ratio.clamp(0.0, 1.0) * 0.11;
        state.detail = format!(
            "Classic model sources: {:.1} / {:.1} MiB",
            bytes as f64 / 1_048_576.0,
            MODEL_SOURCE_BYTES as f64 / 1_048_576.0
        );
    } else if state.stage == "Extracting verified original-US source documents" {
        let ready = count_files_with_extension(&original_us_art_root().join("scans"), "pdf");
        state.amount = 0.60 + (ready.min(16) as f32 / 16.0) * 0.04;
        state.detail = format!("Source documents extracted: {}/16", ready.min(16));
    } else if state.stage == "Rendering board, cards, manuals, and quest pages" {
        let ready = rendered_page_count();
        state.amount = 0.64 + (ready.min(70) as f32 / 70.0) * 0.05;
        state.detail = format!(
            "High-resolution document pages rendered: {}/70",
            ready.min(70)
        );
    } else if state.stage == "Extracting optimized tabletop textures" {
        let ready = extracted_texture_count();
        state.amount = 0.69 + (ready.min(89) as f32 / 89.0) * 0.03;
        state.detail = format!("Runtime tabletop textures extracted: {}/89", ready.min(89));
    } else if state.stage == "Converting classic figures, furniture, traps, and walls" {
        let ready =
            recursive_file_count_with_extension(&original_us_art_root().join("models"), "glb");
        state.amount = 0.86 + (ready.min(33) as f32 / 33.0) * 0.08;
        state.detail = format!("Classic GLB models converted: {}/33", ready.min(33));
    }
    state
}

fn rendered_page_count() -> usize {
    let root = original_us_art_root();
    usize::from(root.join("board-scan.jpg").is_file())
        + usize::from(root.join("board-runtime.jpg").is_file())
        + count_files_with_extension(&root.join("card-sheets"), "png")
        + count_files_with_extension(&root.join("quest-pages"), "png")
        + count_files_with_extension(&root.join("rulebook-pages"), "png")
        + count_files_with_extension(&root.join("tile-sheets"), "png")
        + count_files_with_extension(&root.join("box-pages"), "png")
        + count_files_with_extension(&root.join("character-sheet"), "png")
        + count_files_with_extension(&root.join("armory-pages"), "png")
        + count_files_with_extension(&root.join("poster-pages"), "png")
        + count_files_with_extension(&root.join("extras-pages"), "png")
}

fn extracted_texture_count() -> usize {
    let root = original_us_art_root();
    [
        ("startup/box", "jpg"),
        ("startup/heroes", "jpg"),
        ("startup/quests", "jpg"),
        ("tabletop/player", "jpg"),
        ("tabletop/player", "png"),
        ("tabletop/spells", "jpg"),
        ("tabletop/zargon", "jpg"),
        ("tabletop/monsters", "jpg"),
        ("tabletop/quest-book", "jpg"),
        ("screen", "png"),
        ("dice", "png"),
        ("components/doors", "png"),
        ("components/furniture", "png"),
        ("components/markers", "png"),
    ]
    .into_iter()
    .map(|(directory, extension)| count_files_with_extension(&root.join(directory), extension))
    .sum()
}

fn count_files_with_extension(root: &Path, extension: &str) -> usize {
    let Ok(entries) = fs::read_dir(root) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|candidate| candidate.eq_ignore_ascii_case(extension))
        })
        .count()
}

fn recursive_file_count_with_extension(root: &Path, extension: &str) -> usize {
    let Ok(entries) = fs::read_dir(root) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| {
            entry.metadata().map_or(0, |metadata| {
                if metadata.is_dir() {
                    recursive_file_count_with_extension(&entry.path(), extension)
                } else {
                    usize::from(
                        entry
                            .path()
                            .extension()
                            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(extension)),
                    )
                }
            })
        })
        .sum()
}

fn directory_size(root: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(root) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| {
            entry.metadata().map_or(0, |metadata| {
                if metadata.is_dir() {
                    directory_size(&entry.path())
                } else {
                    metadata.len()
                }
            })
        })
        .sum()
}

fn installer_failure(status: ExitStatus, log_path: &Path) -> String {
    let text = fs::read_to_string(log_path)
        .unwrap_or_default()
        .replace('\r', "\n");
    let useful = text
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && !line.chars().all(|ch| {
                    ch == '#'
                        || ch.is_ascii_digit()
                        || ch == '.'
                        || ch == '%'
                        || ch.is_ascii_whitespace()
                })
        })
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    if useful.is_empty() {
        format!("The complete asset installer exited with {status}.")
    } else {
        format!("The complete asset installer exited with {status}.\n\n{useful}")
    }
}

fn wait_for_retry(
    canvas: &mut WindowCanvas,
    events: &mut EventPump,
    font: &Font,
    error: &str,
) -> Result<bool> {
    render_error(canvas, font, error)?;
    loop {
        for event in events.poll_iter() {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => return Ok(false),
                Event::KeyDown {
                    keycode: Some(Keycode::Return),
                    repeat: false,
                    ..
                } => return Ok(true),
                Event::MouseButtonDown {
                    mouse_btn: MouseButton::Left,
                    x,
                    y,
                    ..
                } if RETRY_BUTTON.contains(x, y) => return Ok(true),
                Event::MouseButtonDown {
                    mouse_btn: MouseButton::Left,
                    x,
                    y,
                    ..
                } if QUIT_BUTTON.contains(x, y) => return Ok(false),
                _ => {}
            }
        }
        thread::sleep(Duration::from_millis(16));
    }
}

fn render_consent(canvas: &mut WindowCanvas, font: &Font) -> Result<()> {
    let mut frame = base_frame();
    draw_text(
        &mut frame,
        font,
        "HeroQuest 3D",
        64,
        42,
        42.0,
        Rgba([238, 201, 111, 255]),
    );
    draw_text(
        &mut frame,
        font,
        "Original US Asset Setup",
        66,
        92,
        27.0,
        Rgba([246, 237, 207, 255]),
    );
    panel(&mut frame, 58, 140, 844, 332);
    draw_text(
        &mut frame,
        font,
        "Required external downloads - about 2.45 GiB",
        90,
        168,
        24.0,
        Rgba([238, 201, 111, 255]),
    );
    let body = "Original-US board, card, manual, quest, box and component scans come directly from heroquestadventure.com.\nClassic figure, monster and furniture STLs come directly from their public Google Drive folder.\nThe project does not host either collection. About 7 GiB is required; interrupted downloads resume automatically.";
    draw_wrapped_text(
        &mut frame,
        font,
        body,
        90,
        208,
        780.0,
        18.0,
        25,
        Rgba([224, 217, 197, 255]),
    );
    draw_wrapped_text(
        &mut frame,
        font,
        "HeroQuest artwork and trademarks belong to their owners. By continuing, you accept responsibility for local download and use under applicable law and source-site terms. This fan project is not affiliated with Hasbro or Avalon Hill.",
        90,
        354,
        780.0,
        16.0,
        22,
        Rgba([184, 177, 159, 255]),
    );
    button(&mut frame, font, QUIT_BUTTON, "Quit", false);
    button(&mut frame, font, ACCEPT_BUTTON, "I Accept - Download", true);
    present(canvas, frame)
}

fn render_progress(canvas: &mut WindowCanvas, font: &Font, state: &ProgressState) -> Result<()> {
    let mut frame = base_frame();
    draw_text(
        &mut frame,
        font,
        "Preparing HeroQuest",
        64,
        46,
        38.0,
        Rgba([238, 201, 111, 255]),
    );
    draw_text(
        &mut frame,
        font,
        "Complete original-US asset installation",
        66,
        96,
        23.0,
        Rgba([222, 211, 184, 255]),
    );
    panel(&mut frame, 58, 154, 844, 304);
    draw_wrapped_text(
        &mut frame,
        font,
        &state.stage,
        90,
        192,
        760.0,
        27.0,
        34,
        Rgba([246, 237, 207, 255]),
    );
    draw_wrapped_text(
        &mut frame,
        font,
        &state.detail,
        90,
        260,
        760.0,
        19.0,
        26,
        Rgba([190, 183, 164, 255]),
    );
    fill_rect(&mut frame, 90, 336, 760, 42, Rgba([20, 18, 18, 255]));
    outline_rect(&mut frame, 90, 336, 760, 42, 2, Rgba([129, 94, 46, 255]));
    let fill = (752.0 * state.amount.clamp(0.0, 1.0)).round() as u32;
    fill_rect(&mut frame, 94, 340, fill, 34, Rgba([145, 46, 28, 255]));
    fill_rect(&mut frame, 94, 340, fill, 5, Rgba([226, 151, 66, 255]));
    let percent = format!(
        "{:>3}%",
        (state.amount.clamp(0.0, 1.0) * 100.0).round() as u32
    );
    draw_text(
        &mut frame,
        font,
        &percent,
        792,
        391,
        22.0,
        Rgba([238, 201, 111, 255]),
    );
    draw_text(
        &mut frame,
        font,
        "Downloads are verified before any source is converted.",
        90,
        412,
        17.0,
        Rgba([153, 146, 130, 255]),
    );
    button(&mut frame, font, CANCEL_BUTTON, "Cancel", false);
    present(canvas, frame)
}

fn render_error(canvas: &mut WindowCanvas, font: &Font, error: &str) -> Result<()> {
    let mut frame = base_frame();
    draw_text(
        &mut frame,
        font,
        "Installation Paused",
        64,
        46,
        38.0,
        Rgba([238, 201, 111, 255]),
    );
    draw_text(
        &mut frame,
        font,
        "No completed assets were removed",
        66,
        96,
        23.0,
        Rgba([222, 211, 184, 255]),
    );
    panel(&mut frame, 58, 154, 844, 304);
    draw_wrapped_text(
        &mut frame,
        font,
        error,
        90,
        190,
        760.0,
        18.0,
        25,
        Rgba([239, 205, 190, 255]),
    );
    draw_wrapped_text(
        &mut frame,
        font,
        "Partial downloads are retained. Retry resumes them instead of starting over.",
        90,
        410,
        760.0,
        18.0,
        25,
        Rgba([184, 177, 159, 255]),
    );
    button(&mut frame, font, QUIT_BUTTON, "Quit", false);
    button(&mut frame, font, RETRY_BUTTON, "Retry Installation", true);
    present(canvas, frame)
}

fn base_frame() -> RgbaImage {
    let mut frame = RgbaImage::new(WIDTH, HEIGHT);
    for y in 0..HEIGHT {
        let shade = 24_u8.saturating_sub((y * 12 / HEIGHT) as u8);
        for x in 0..WIDTH {
            let edge = x.min(WIDTH - 1 - x).min(y).min(HEIGHT - 1 - y);
            let vignette = if edge < 90 {
                ((90 - edge) / 8) as u8
            } else {
                0
            };
            frame.put_pixel(
                x,
                y,
                Rgba([
                    shade.saturating_add(10).saturating_sub(vignette),
                    shade.saturating_add(5).saturating_sub(vignette),
                    shade.saturating_sub(vignette),
                    255,
                ]),
            );
        }
    }
    outline_rect(
        &mut frame,
        18,
        18,
        WIDTH - 36,
        HEIGHT - 36,
        2,
        Rgba([111, 76, 35, 255]),
    );
    outline_rect(
        &mut frame,
        25,
        25,
        WIDTH - 50,
        HEIGHT - 50,
        1,
        Rgba([58, 43, 28, 255]),
    );
    frame
}

fn panel(frame: &mut RgbaImage, x: u32, y: u32, width: u32, height: u32) {
    fill_rect(frame, x, y, width, height, Rgba([31, 28, 27, 245]));
    outline_rect(frame, x, y, width, height, 2, Rgba([104, 72, 35, 255]));
    outline_rect(
        frame,
        x + 7,
        y + 7,
        width - 14,
        height - 14,
        1,
        Rgba([57, 44, 31, 255]),
    );
}

fn button(frame: &mut RgbaImage, font: &Font, rect: UiRect, label: &str, primary: bool) {
    let fill = if primary {
        Rgba([118, 37, 25, 255])
    } else {
        Rgba([51, 46, 42, 255])
    };
    let border = if primary {
        Rgba([222, 151, 65, 255])
    } else {
        Rgba([114, 91, 60, 255])
    };
    fill_rect(frame, rect.x, rect.y, rect.width, rect.height, fill);
    outline_rect(frame, rect.x, rect.y, rect.width, rect.height, 2, border);
    let size = 22.0;
    let width = text_width(font, label, size);
    draw_text(
        frame,
        font,
        label,
        (rect.x as f32 + (rect.width as f32 - width) * 0.5).round() as i32,
        rect.y as i32 + 17,
        size,
        Rgba([246, 237, 207, 255]),
    );
}

fn fill_rect(frame: &mut RgbaImage, x: u32, y: u32, width: u32, height: u32, color: Rgba<u8>) {
    for py in y..(y + height).min(frame.height()) {
        for px in x..(x + width).min(frame.width()) {
            frame.put_pixel(px, py, color);
        }
    }
}

fn outline_rect(
    frame: &mut RgbaImage,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    thickness: u32,
    color: Rgba<u8>,
) {
    fill_rect(frame, x, y, width, thickness, color);
    fill_rect(frame, x, y + height - thickness, width, thickness, color);
    fill_rect(frame, x, y, thickness, height, color);
    fill_rect(frame, x + width - thickness, y, thickness, height, color);
}

fn text_width(font: &Font, text: &str, size: f32) -> f32 {
    text.chars()
        .map(|character| font.metrics(character, size).advance_width)
        .sum()
}

fn wrap_text(font: &Font, text: &str, size: f32, max_width: f32) -> String {
    let mut output = Vec::new();
    for paragraph in text.lines() {
        if paragraph.is_empty() {
            output.push(String::new());
            continue;
        }
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let candidate = if line.is_empty() {
                word.to_owned()
            } else {
                format!("{line} {word}")
            };
            if !line.is_empty() && text_width(font, &candidate, size) > max_width {
                output.push(std::mem::take(&mut line));
                line.push_str(word);
            } else {
                line = candidate;
            }
        }
        if !line.is_empty() {
            output.push(line);
        }
    }
    output.join("\n")
}

#[allow(clippy::too_many_arguments)]
fn draw_wrapped_text(
    frame: &mut RgbaImage,
    font: &Font,
    text: &str,
    x: i32,
    y: i32,
    max_width: f32,
    size: f32,
    line_advance: i32,
    color: Rgba<u8>,
) {
    let wrapped = wrap_text(font, text, size, max_width);
    draw_text_with_advance(frame, font, &wrapped, x, y, size, line_advance, color);
}

fn draw_text(
    frame: &mut RgbaImage,
    font: &Font,
    text: &str,
    x: i32,
    y: i32,
    size: f32,
    color: Rgba<u8>,
) {
    draw_text_with_advance(
        frame,
        font,
        text,
        x,
        y,
        size,
        (size * 1.35).round() as i32,
        color,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_text_with_advance(
    frame: &mut RgbaImage,
    font: &Font,
    text: &str,
    x: i32,
    y: i32,
    size: f32,
    line_advance: i32,
    color: Rgba<u8>,
) {
    let ascent = font
        .horizontal_line_metrics(size)
        .map_or(size * 0.8, |metrics| metrics.ascent);
    let mut cursor_x = x as f32;
    let mut cursor_y = y;
    for character in text.chars() {
        if character == '\n' {
            cursor_x = x as f32;
            cursor_y += line_advance;
            continue;
        }
        let (metrics, bitmap) = font.rasterize(character, size);
        let baseline = cursor_y + ascent.ceil() as i32;
        let glyph_x = cursor_x.round() as i32 + metrics.xmin;
        let glyph_y = baseline - metrics.ymin - metrics.height as i32;
        for row in 0..metrics.height {
            for column in 0..metrics.width {
                let coverage = bitmap[row * metrics.width + column] as u32;
                if coverage == 0 {
                    continue;
                }
                let px = glyph_x + column as i32;
                let py = glyph_y + row as i32;
                if px < 0 || py < 0 || px >= frame.width() as i32 || py >= frame.height() as i32 {
                    continue;
                }
                let destination = *frame.get_pixel(px as u32, py as u32);
                let source_alpha = color[3] as u32 * coverage / 255;
                let inverse_alpha = 255 - source_alpha;
                let mut blended = [0_u8; 4];
                for channel in 0..3 {
                    blended[channel] = ((color[channel] as u32 * source_alpha
                        + destination[channel] as u32 * inverse_alpha)
                        / 255) as u8;
                }
                blended[3] =
                    (source_alpha + destination[3] as u32 * inverse_alpha / 255).min(255) as u8;
                frame.put_pixel(px as u32, py as u32, Rgba(blended));
            }
        }
        cursor_x += metrics.advance_width;
    }
}

fn present(canvas: &mut WindowCanvas, frame: RgbaImage) -> Result<()> {
    let mut pixels = frame.into_raw();
    let surface = Surface::from_data(&mut pixels, WIDTH, HEIGHT, WIDTH * 4, PixelFormat::RGBA32)
        .map_err(|error| anyhow!(error.to_string()))?;
    let texture_creator = canvas.texture_creator();
    let texture = texture_creator
        .create_texture_from_surface(&surface)
        .map_err(|error| anyhow!(error.to_string()))?;
    canvas
        .copy(&texture, None, None)
        .map_err(|error| anyhow!(error.to_string()))?;
    if !canvas.present() {
        return Err(anyhow!(sdl3::get_error().to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_protocol_uses_the_last_complete_record() {
        let path = std::env::temp_dir().join(format!(
            "heroquest-progress-test-{}-{}.txt",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&path, "0.02\tDownloading scans\n0.64\tRendering pages\n").unwrap();
        let progress = read_progress(&path).unwrap();
        let _ = fs::remove_file(path);
        assert_eq!(progress.stage, "Rendering pages");
        assert!((progress.amount - 0.64).abs() < f32::EPSILON);
    }

    #[test]
    fn setup_copy_wraps_within_the_requested_width() {
        let font = Font::from_bytes(
            include_bytes!("../assets/fonts/Almendra-Regular.ttf") as &[u8],
            fontdue::FontSettings::default(),
        )
        .unwrap();
        let wrapped = wrap_text(
            &font,
            "This is a deliberately long first-run installation sentence.",
            20.0,
            180.0,
        );
        assert!(wrapped.lines().count() > 1);
        assert!(
            wrapped
                .lines()
                .all(|line| text_width(&font, line, 20.0) <= 180.0)
        );
    }
}
