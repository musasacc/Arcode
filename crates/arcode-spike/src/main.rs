//! Step 0 spike: can GPUI open and smoothly scroll a 100k-line file?
//!
//! Usage:
//!   arcode-spike [FILE]            interactive; generates 100k lines if no FILE
//!   arcode-spike --bench [FILE]    auto-scrolls through the file, prints frame
//!                                  stats and GPU info, then quits
//!
//! Frame timing is measured between consecutive `render` calls, so in bench
//! mode the numbers are vsync-bound on real hardware. In interactive mode GPUI
//! only renders when something is dirty: an idle window costs ~0% CPU.

use std::time::{Duration, Instant};

use gpui::{
    App, Application, Bounds, Context, ScrollStrategy, SharedString, UniformListScrollHandle,
    Window, WindowBounds, WindowOptions, div, prelude::*, px, rgb, size, uniform_list,
};

const GENERATED_LINES: usize = 100_000;
const BENCH_FRAMES: usize = 600;
const BENCH_LINES_PER_FRAME: usize = 150;

fn mono_font() -> &'static str {
    if cfg!(target_os = "macos") {
        "Menlo"
    } else if cfg!(target_os = "windows") {
        "Consolas"
    } else {
        "DejaVu Sans Mono"
    }
}

fn generate_lines(n: usize) -> Vec<SharedString> {
    (0..n)
        .map(|i| match i % 6 {
            0 => format!("fn function_{i}(x: u64) -> u64 {{"),
            1 => format!("    let value = x.wrapping_mul({i}) ^ 0x9e37_79b9;"),
            2 => "    // a comment line to vary glyph mix: àéîõü 漢字 ✓".to_string(),
            3 => format!("    if value % 2 == 0 {{ return value + {i}; }}"),
            4 => "    value".to_string(),
            _ => "}".to_string(),
        })
        .map(SharedString::from)
        .collect()
}

fn load_lines(path: &str) -> std::io::Result<Vec<SharedString>> {
    let text = std::fs::read_to_string(path)?;
    Ok(text
        .lines()
        .map(|l| SharedString::from(l.to_string()))
        .collect())
}

struct Bench {
    frame_times: Vec<Duration>,
    last_frame: Option<Instant>,
    next_line: usize,
}

struct Spike {
    lines: Vec<SharedString>,
    scroll: UniformListScrollHandle,
    started: Instant,
    first_frame: Option<Duration>,
    renders: u64,
    bench: Option<Bench>,
}

impl Spike {
    fn tick_bench(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bench) = self.bench.as_mut() else {
            return;
        };
        let now = Instant::now();
        if let Some(last) = bench.last_frame {
            bench.frame_times.push(now - last);
        }
        bench.last_frame = Some(now);

        if bench.frame_times.len() >= BENCH_FRAMES {
            report(
                &bench.frame_times,
                self.first_frame,
                self.lines.len(),
                window,
            );
            cx.quit();
            return;
        }
        bench.next_line = (bench.next_line + BENCH_LINES_PER_FRAME) % self.lines.len();
        self.scroll
            .scroll_to_item(bench.next_line, ScrollStrategy::Top);
        window.request_animation_frame();
    }
}

fn report(times: &[Duration], first_frame: Option<Duration>, lines: usize, window: &Window) {
    let mut ms: Vec<f64> = times.iter().map(|d| d.as_secs_f64() * 1000.0).collect();
    ms.sort_by(f64::total_cmp);
    let pct = |p: f64| ms[((ms.len() - 1) as f64 * p).round() as usize];
    let avg = ms.iter().sum::<f64>() / ms.len() as f64;
    println!("arcode-spike bench");
    println!("  lines:            {lines}");
    println!("  frames:           {}", ms.len());
    println!(
        "  first frame:      {:.1} ms after start",
        first_frame.unwrap_or_default().as_secs_f64() * 1000.0
    );
    println!("  frame avg:        {avg:.2} ms ({:.0} fps)", 1000.0 / avg);
    println!(
        "  frame p50/p95/p99:{:.2} / {:.2} / {:.2} ms",
        pct(0.5),
        pct(0.95),
        pct(0.99)
    );
    println!("  frame max:        {:.2} ms", ms[ms.len() - 1]);
    match window.gpu_specs() {
        Some(gpu) => println!(
            "  gpu:              {} (driver: {} {}) software={}",
            gpu.device_name, gpu.driver_name, gpu.driver_info, gpu.is_software_emulated
        ),
        None => println!("  gpu:              <not reported by platform>"),
    }
}

impl Render for Spike {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;
        if std::env::var_os("ARCODE_SPIKE_TRACE").is_some() {
            eprintln!("render #{} at {:?}", self.renders, self.started.elapsed());
        }
        if self.first_frame.is_none() {
            self.first_frame = Some(self.started.elapsed());
        }
        self.tick_bench(window, cx);

        let gutter_width = self.lines.len().to_string().len();
        let status: SharedString = format!(
            "{} lines · first frame {:.0} ms · renders {}",
            self.lines.len(),
            self.first_frame.unwrap_or_default().as_secs_f64() * 1000.0,
            self.renders
        )
        .into();

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x1e1f22))
            .text_color(rgb(0xbcbec4))
            .font_family(mono_font())
            .text_size(px(13.0))
            .child(
                uniform_list(
                    "lines",
                    self.lines.len(),
                    cx.processor(move |this, range: std::ops::Range<usize>, _window, _cx| {
                        range
                            .map(|ix| {
                                div()
                                    .id(ix)
                                    .flex()
                                    .h(px(20.0))
                                    .child(
                                        div()
                                            .w(px(gutter_width as f32 * 8.0 + 16.0))
                                            .pr_2()
                                            .flex()
                                            .justify_end()
                                            .text_color(rgb(0x4b5059))
                                            .child(SharedString::from((ix + 1).to_string())),
                                    )
                                    .child(this.lines[ix].clone())
                            })
                            .collect()
                    }),
                )
                .track_scroll(self.scroll.clone())
                .flex_grow(),
            )
            .child(
                div()
                    .h(px(24.0))
                    .px_2()
                    .flex()
                    .items_center()
                    .bg(rgb(0x2b2d30))
                    .text_size(px(12.0))
                    .child(status),
            )
    }
}

fn main() {
    let started = Instant::now();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let bench = if let Some(pos) = args.iter().position(|a| a == "--bench") {
        args.remove(pos);
        true
    } else {
        false
    };
    let lines = match args.first() {
        Some(path) => load_lines(path).unwrap_or_else(|e| {
            eprintln!("failed to read {path}: {e}");
            std::process::exit(1);
        }),
        None => generate_lines(GENERATED_LINES),
    };

    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1000.0), px(700.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            move |_, cx| {
                cx.new(|_| Spike {
                    lines,
                    scroll: UniformListScrollHandle::new(),
                    started,
                    first_frame: None,
                    renders: 0,
                    bench: bench.then(|| Bench {
                        frame_times: Vec::with_capacity(BENCH_FRAMES),
                        last_frame: None,
                        next_line: 0,
                    }),
                })
            },
        )
        .expect("failed to open window");
        cx.activate(true);
    });
}
