//! The custom element that lays out and paints the terminal grid.

use arcode_terminal::{Cell, CursorShape, Rgb, Snapshot};
use arcode_ui_kit::{apply_glyph_fallbacks, settings, theme};
use gpui::{
    App, BorderStyle, Bounds, ContentMask, DispatchPhase, Element, ElementId, ElementInputHandler,
    Entity, Font, FontFallbacks, FontFeatures, FontStyle, FontWeight, GlobalElementId, Hsla,
    InspectorElementId, IntoElement, LayoutId, MouseMoveEvent, PaintQuad, Pixels, Point,
    ShapedLine, SharedString, StrikethroughStyle, Style, TextRun, UnderlineStyle, Window, fill,
    font, outline, point, px, relative, size,
};

use crate::colors::rgb_to_hsla;
use crate::layout::{CellStyle, background_runs, grid_size, selection_runs, text_segments};
use crate::view::{FrameInfo, TerminalView};

/// Font and cell metrics for one frame.
struct Metrics {
    font: Font,
    font_size: Pixels,
    cell_width: Pixels,
    line_height: Pixels,
}

/// Terminal line height relative to the font size. The editor's spacing
/// is honored but capped so box-drawing characters still connect.
const MAX_LINE_HEIGHT: f32 = 1.4;

fn metrics(window: &Window, cx: &App) -> Metrics {
    let config = settings(cx);
    let font_size = px(config.terminal_font_size());
    let mut font = font(config.terminal_font_family().to_owned());
    if !config.editor.font_fallbacks.is_empty() {
        font.fallbacks = Some(FontFallbacks::from_fonts(
            config.editor.font_fallbacks.clone(),
        ));
    }
    if !config.editor.ligatures {
        font.features = FontFeatures::disable_ligatures();
    }
    let text = window.text_system();
    let id = text.resolve_font(&font);
    let cell_width = text
        .advance(id, font_size, 'm')
        .map(|s| s.width)
        .or_else(|_| text.em_advance(id, font_size))
        .unwrap_or(font_size * 0.6);
    let factor = config.editor.line_height.clamp(1.0, MAX_LINE_HEIGHT);
    Metrics {
        font,
        font_size,
        cell_width: if cell_width > px(0.) {
            cell_width
        } else {
            font_size * 0.6
        },
        line_height: (font_size * factor).round().max(px(1.)),
    }
}

/// Everything to paint, computed in prepaint.
pub(crate) struct Frame {
    /// Cell backgrounds, selection and a filled block cursor.
    under: Vec<PaintQuad>,
    lines: Vec<(Point<Pixels>, ShapedLine)>,
    /// Hollow/beam/underline cursor and IME preedit background.
    over: Vec<PaintQuad>,
    preedit: Option<(Point<Pixels>, ShapedLine)>,
    line_height: Pixels,
}

/// Paints a [`TerminalView`]'s grid and resizes the PTY to fit.
pub(crate) struct TerminalElement {
    view: Entity<TerminalView>,
    focused: bool,
}

impl TerminalElement {
    pub(crate) fn new(view: Entity<TerminalView>, focused: bool) -> Self {
        Self { view, focused }
    }
}

impl IntoElement for TerminalElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// Pixel size as whole pixels for the PTY's cell size report.
fn whole_px(p: Pixels) -> u16 {
    f32::from(p).round().clamp(1., f32::from(u16::MAX)) as u16
}

impl Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = Frame;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let m = metrics(window, cx);
        let (cols, rows) = grid_size(
            bounds.size.width.into(),
            bounds.size.height.into(),
            m.cell_width.into(),
            m.line_height.into(),
        );
        let size_key = (cols, rows, whole_px(m.cell_width), whole_px(m.line_height));
        let selection = theme(cx).colors.selection;
        let view = self.view.read(cx);
        if view.applied_size != Some(size_key) {
            view.terminal
                .resize(size_key.0, size_key.1, size_key.2, size_key.3);
        }
        let snapshot = view.terminal.snapshot(&view.palette);
        let default_bg = view.palette.background;
        let preedit = view.marked_text.clone();

        let ctx = FrameBuilder {
            m: &m,
            origin: bounds.origin,
            focused: self.focused,
            default_fg: view.palette.foreground,
            default_bg,
            selection,
        };
        let frame = ctx.build(&snapshot, preedit, window);
        let info = FrameInfo {
            origin: bounds.origin,
            cell_width: m.cell_width,
            line_height: m.line_height,
            cols: snapshot.cols,
            rows: snapshot.rows,
            cursor: snapshot.cursor.map(|c| (c.row, c.col)),
            display_offset: snapshot.display_offset,
        };
        self.view.update(cx, |view, _| {
            view.applied_size = Some(size_key);
            view.frame = Some(info);
        });
        frame
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        frame: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let view = self.view.read(cx);
        let focus_handle = view.focus_handle.clone();
        let dragging = view.drag.is_some();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.view.clone()),
            cx,
        );
        if dragging {
            // Keep extending the selection when the pointer leaves the grid.
            let view = self.view.clone();
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble && e.dragging() {
                    view.update(cx, |view, cx| view.on_mouse_move(e, window, cx));
                }
            });
        }
        let line_height = frame.line_height;
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for quad in frame.under.drain(..) {
                window.paint_quad(quad);
            }
            for (origin, line) in &frame.lines {
                if let Err(err) = line.paint(*origin, line_height, window, cx) {
                    log::debug!("terminal: failed to paint a line: {err:#}");
                }
            }
            for quad in frame.over.drain(..) {
                window.paint_quad(quad);
            }
            if let Some((origin, line)) = &frame.preedit
                && let Err(err) = line.paint(*origin, line_height, window, cx)
            {
                log::debug!("terminal: failed to paint preedit text: {err:#}");
            }
        });
    }
}

/// Builds a [`Frame`] from a snapshot.
struct FrameBuilder<'a> {
    m: &'a Metrics,
    origin: Point<Pixels>,
    focused: bool,
    default_fg: Rgb,
    default_bg: Rgb,
    selection: Hsla,
}

impl FrameBuilder<'_> {
    fn cell_bounds(&self, row: usize, col: usize, width_cells: usize) -> Bounds<Pixels> {
        Bounds::new(
            point(
                self.origin.x + self.m.cell_width * col as f32,
                self.origin.y + self.m.line_height * row as f32,
            ),
            size(self.m.cell_width * width_cells as f32, self.m.line_height),
        )
    }

    fn build(&self, snapshot: &Snapshot, preedit: Option<String>, window: &Window) -> Frame {
        let mut frame = Frame {
            under: Vec::new(),
            lines: Vec::with_capacity(snapshot.rows),
            over: Vec::new(),
            preedit: None,
            line_height: self.m.line_height,
        };
        // A focused block cursor is a filled cell with the glyph drawn in
        // the cell's background color: (row, col, cell bounds, color, glyph color).
        let mut block = None;
        if let Some(c) = snapshot.cursor.filter(|_| preedit.is_none()) {
            let cell = snapshot.cell(c.row, c.col);
            let width = if cell.is_some_and(|cell| cell.wide) {
                2
            } else {
                1
            };
            let b = self.cell_bounds(c.row, c.col, width);
            let color = rgb_to_hsla(c.color);
            let thin = px(2.).min(self.m.cell_width);
            match (c.shape, self.focused) {
                (CursorShape::Block, true) => {
                    let glyph = cell.map_or(self.default_bg, |cell| cell.bg);
                    block = Some((c.row, c.col, b, color, glyph));
                }
                (CursorShape::Block, false) => {
                    frame.over.push(outline(b, color, BorderStyle::Solid));
                }
                (CursorShape::Beam, _) => {
                    let bar = Bounds::new(b.origin, size(thin, b.size.height));
                    frame.over.push(fill(bar, color));
                }
                (CursorShape::Underline, _) => {
                    let y = b.origin.y + b.size.height - thin;
                    let bar = Bounds::new(point(b.origin.x, y), size(b.size.width, thin));
                    frame.over.push(fill(bar, color));
                }
            }
        }

        for (row, cells) in snapshot.row_iter().enumerate() {
            for (range, bg) in background_runs(cells, self.default_bg) {
                let b = self.cell_bounds(row, range.start, range.len());
                frame.under.push(fill(b, rgb_to_hsla(bg)));
            }
            for range in selection_runs(cells) {
                let b = self.cell_bounds(row, range.start, range.len());
                frame.under.push(fill(b, self.selection));
            }
            let mut fg_override = None;
            if let Some((r, col, b, color, glyph)) = block
                && r == row
            {
                frame.under.push(fill(b, color));
                fg_override = Some((col, glyph));
            }
            self.shape_row(row, cells, fg_override, window, &mut frame);
        }

        if let (Some(text), Some(c)) = (preedit, snapshot.cursor) {
            frame.preedit = Some(self.shape_preedit(text, c.row, c.col, window, &mut frame.over));
        }
        frame
    }

    fn shape_row(
        &self,
        row: usize,
        cells: &[Cell],
        fg_override: Option<(usize, Rgb)>,
        window: &Window,
        frame: &mut Frame,
    ) {
        let fallbacks: Vec<SharedString> = self
            .m
            .font
            .fallbacks
            .as_ref()
            .map(|f| f.fallback_list().iter().map(|s| s.clone().into()).collect())
            .unwrap_or_default();
        for seg in text_segments(cells, fg_override) {
            let mut runs: Vec<TextRun> = seg
                .runs
                .iter()
                .map(|run| self.text_run(run.len, &run.style))
                .collect();
            // GPUI's Linux backend ignores `Font::fallbacks` and cannot fall
            // back from italic runs; split out glyphs the font lacks.
            if let Some(split) =
                apply_glyph_fallbacks(&seg.text, &runs, &fallbacks, window.text_system())
            {
                runs = split;
            }
            let force_width = (!seg.single).then_some(self.m.cell_width);
            let line = window.text_system().shape_line(
                SharedString::from(seg.text),
                self.m.font_size,
                &runs,
                force_width,
            );
            frame
                .lines
                .push((self.cell_bounds(row, seg.col, 1).origin, line));
        }
    }

    fn text_run(&self, len: usize, style: &CellStyle) -> TextRun {
        let mut font = self.m.font.clone();
        if style.bold {
            font.weight = FontWeight::BOLD;
        }
        if style.italic {
            font.style = FontStyle::Italic;
        }
        let color = rgb_to_hsla(style.fg);
        TextRun {
            len,
            font,
            color,
            background_color: None,
            underline: style.underline.then_some(UnderlineStyle {
                thickness: px(1.),
                color: Some(color),
                wavy: false,
            }),
            strikethrough: style.strikeout.then_some(StrikethroughStyle {
                thickness: px(1.),
                color: Some(color),
            }),
        }
    }

    /// Shapes IME composition text at the cursor over a background box.
    fn shape_preedit(
        &self,
        text: String,
        row: usize,
        col: usize,
        window: &Window,
        over: &mut Vec<PaintQuad>,
    ) -> (Point<Pixels>, ShapedLine) {
        let style = CellStyle {
            fg: self.default_fg,
            bold: false,
            italic: false,
            underline: true,
            strikeout: false,
        };
        let run = self.text_run(text.len(), &style);
        let line = window.text_system().shape_line(
            SharedString::from(text),
            self.m.font_size,
            &[run],
            None,
        );
        let origin = self.cell_bounds(row, col, 1).origin;
        let width = line.width.max(self.m.cell_width);
        over.push(fill(
            Bounds::new(origin, size(width, self.m.line_height)),
            rgb_to_hsla(self.default_bg),
        ));
        (origin, line)
    }
}
