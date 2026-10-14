//! Shared pieces of the canforge UI: the responsive layout the window width selects, a few small
//! widgets (status labels, tags, banners, segmented controls, chip buttons), the line plot used for
//! sparklines and the combined chart, and the overlay helper that is a dialog on a laptop and a
//! bottom sheet on touch layouts.
//!
//! This is a child module of `main`, so it can use the `Diagnostics` view's state directly.

use super::*;

use crate::i18n::tr;

/// Minimum size of anything tappable in the touch layouts.
pub const TAP: f32 = 44.;

/// Which of the design's three layouts the window is wide enough for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout {
    /// A sidebar of 208 px, pointer-sized controls.
    Laptop,
    /// Tablet landscape: a 92 px icon rail, touch-sized controls.
    Tablet,
    /// Tablet portrait: a bottom tab bar, touch-sized controls.
    Portrait,
}

impl Layout {
    /// The layout for a window this wide. The manual tablet mode keeps a wide window in the touch
    /// layout, so it still does something.
    pub fn detect(width: f32) -> Layout {
        if width <= 820. {
            Layout::Portrait
        } else if width <= 1180. || tablet() {
            Layout::Tablet
        } else {
            Layout::Laptop
        }
    }

    pub fn touch(self) -> bool {
        self != Layout::Laptop
    }

    /// Side padding of the page content.
    pub fn gutter(self) -> f32 {
        if self.touch() { 20. } else { 24. }
    }

    /// Height of a button.
    pub fn button(self) -> Pixels {
        px(if self.touch() { TAP } else { 36. })
    }
}

/// A font size from the design's pixel value, in rems so the text-size setting scales it.
pub fn r(design_px: f32) -> Rems {
    rems(design_px / 14.)
}

/// Icons the kit does not ship, drawn from the files embedded here; everything else comes from the
/// kit's own assets.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        match path {
            "icons/clock.svg" => Ok(Some(std::borrow::Cow::Borrowed(include_bytes!("../assets/icons/clock.svg")))),
            _ => gpui_kit::assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> gpui_kit::Result<Vec<SharedString>> {
        gpui_kit::assets::Assets.list(path)
    }
}

pub fn clock_icon() -> Icon {
    Icon::empty().path("icons/clock.svg")
}

/// What a status or verdict is: the colour and the icon always travel together, so colour is never
/// the only cue.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Ok,
    Warn,
    Bad,
    Muted,
}

impl Tone {
    pub fn color(self, theme: &gpui_kit::component::Theme) -> Hsla {
        match self {
            Tone::Ok => theme.green,
            Tone::Warn => theme.yellow,
            Tone::Bad => theme.red,
            Tone::Muted => theme.muted_foreground,
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Tone::Ok => Icon::new(IconName::CircleCheck),
            Tone::Warn => Icon::new(IconName::TriangleAlert),
            Tone::Bad => Icon::new(IconName::CircleX),
            Tone::Muted => Icon::new(IconName::CircleAlert),
        }
    }
}

/// A fault's status as the design draws it: a clock for pending, an exclamation for stored.
pub fn fault_status(status: &str, size: f32, theme: &gpui_kit::component::Theme) -> Div {
    let pending = status == "pending";
    let (icon, color, label) = if pending {
        (clock_icon(), theme.yellow, tr("pending"))
    } else {
        (Icon::new(IconName::CircleAlert), theme.red, if status == "confirmed" { tr("confirmed") } else { tr("stored") })
    };
    h_flex()
        .gap_1p5()
        .items_center()
        .flex_none()
        .text_color(color)
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(r(12.))
        .child(icon.size(px(size)))
        .child(label)
}

/// A small icon plus text in a tone, with text 14 px tall.
pub fn verdict(tone: Tone, text: impl Into<SharedString>, icon: f32, theme: &gpui_kit::component::Theme) -> Div {
    verdict_at(tone, text, icon, 14., theme)
}

/// An icon beside text of `text_px` pixels. The icon sits in a box as tall as one line of the text,
/// so it is centred on the first line, and stays there when the text wraps onto more lines.
pub fn verdict_at(tone: Tone, text: impl Into<SharedString>, icon: f32, text_px: f32, theme: &gpui_kit::component::Theme) -> Div {
    let line = r(text_px * 1.4);
    h_flex()
        .gap_1p5()
        .items_start()
        .text_color(tone.color(theme))
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(r(text_px))
        .line_height(line)
        .child(div().h(line).flex_none().flex().items_center().child(tone.icon().size(px(icon))))
        .child(div().min_w_0().child(text.into()))
}

pub fn tag(text: impl Into<SharedString>, color: Hsla, theme: &gpui_kit::component::Theme) -> Div {
    div()
        .h(px(18.))
        .px_1p5()
        .flex()
        .items_center()
        .rounded(px(3.))
        .bg(theme.muted)
        .text_color(color)
        .text_size(r(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(text.into())
}

pub fn card(theme: &gpui_kit::component::Theme) -> Div {
    v_flex().gap_2().p_4().border_1().border_color(theme.border).rounded(theme.radius_lg)
}

/// A card that stands out: the one thing to look at.
pub fn focus_card(theme: &gpui_kit::component::Theme) -> Div {
    card(theme).border_color(theme.foreground)
}

pub fn section_title(text: impl Into<SharedString>, theme: &gpui_kit::component::Theme) -> Div {
    div().text_size(r(12.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(text.into().to_uppercase())
}

/// A thin horizontal progress bar (the design's `.pbar` / `.bar`).
pub fn bar(percent: f32, height: f32, color: Hsla, theme: &gpui_kit::component::Theme) -> Div {
    div().w_full().h(px(height)).rounded_full().bg(theme.muted).overflow_hidden().child(div().h_full().w(relative((percent / 100.).clamp(0., 1.))).bg(color))
}

/// One series of a plot: values already scaled to 0..1.
pub struct Series {
    pub values: Vec<f32>,
    /// How many samples the plot's full width holds; the values are right-aligned in it, so a
    /// signal with a short history starts part-way across.
    pub slots: usize,
    pub color: Hsla,
    pub width: f32,
    /// Dash pattern in px; empty is a solid line. Each series gets its own, so colour is not the
    /// only way to tell lines apart.
    pub dash: Vec<f32>,
}

/// Draws the series as lines across the element, with a dashed vertical marker at `marker` (0..1)
/// and horizontal grid lines. GPUI has no SVG path element, so this paints the paths directly.
pub fn plot(series: Vec<Series>, marker: Option<(f32, Hsla)>, grid: Option<Hsla>, inset: f32) -> Canvas<()> {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let (x0, y0) = (f32::from(bounds.origin.x) + inset, f32::from(bounds.origin.y) + 4.);
            let (w, h) = ((f32::from(bounds.size.width) - inset - 4.).max(1.), (f32::from(bounds.size.height) - 8.).max(1.));
            if let Some(color) = grid {
                for i in 0..5 {
                    let y = y0 + h * i as f32 / 4.;
                    let mut p = PathBuilder::stroke(px(1.));
                    p.move_to(point(px(x0), px(y)));
                    p.line_to(point(px(x0 + w), px(y)));
                    if let Ok(path) = p.build() {
                        window.paint_path(path, color);
                    }
                }
            }
            for s in &series {
                let n = s.values.len();
                if n < 2 {
                    continue;
                }
                let mut p = PathBuilder::stroke(px(s.width));
                if !s.dash.is_empty() {
                    let dash: Vec<Pixels> = s.dash.iter().map(|d| px(*d)).collect();
                    p = p.dash_array(&dash);
                }
                for (i, v) in s.values.iter().enumerate() {
                    let slot = (s.slots.max(n) - n + i) as f32 / (s.slots.max(n) - 1) as f32;
                    let pt = point(px(x0 + w * slot), px(y0 + h * (1. - v.clamp(0., 1.))));
                    if i == 0 { p.move_to(pt) } else { p.line_to(pt) }
                }
                if let Ok(path) = p.build() {
                    window.paint_path(path, s.color);
                }
            }
            if let Some((at, color)) = marker {
                let x = x0 + w * at.clamp(0., 1.);
                let mut p = PathBuilder::stroke(px(1.)).dash_array(&[px(3.), px(3.)]);
                p.move_to(point(px(x), px(y0)));
                p.line_to(point(px(x), px(y0 + h)));
                if let Ok(path) = p.build() {
                    window.paint_path(path, color);
                }
            }
        },
    )
}

/// Dash patterns for the combined chart's series, in the order the series are drawn: solid,
/// dashed, dotted, dash-dot, long dash, solid. Matches the design's `.s1`…`.s6`.
pub const DASHES: [&[f32]; 6] = [&[], &[8., 5.], &[2., 4.], &[10., 4., 2., 4.], &[16., 6.], &[]];

impl Diagnostics {
    pub fn lay(&self) -> Layout {
        self.ui.layout
    }

    /// A page's scrolling container with the gutter and gap the layout calls for.
    pub fn page_box(&self, id: &'static str) -> Stateful<Div> {
        let g = self.lay().gutter();
        v_flex().id(id).px(px(g)).pt_4().pb_6().gap_4().size_full().overflow_y_scroll()
    }

    /// A button as tall as the layout needs.
    pub fn sized(&self, b: Button) -> Button {
        b.h(self.lay().button())
    }

    /// A segmented control: one of a few choices, each at least a tap target wide.
    pub fn seg<const N: usize>(
        &self,
        id: &'static str,
        items: [(&'static str, bool, fn(&mut Self, &mut Window, &mut Context<Self>)); N],
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = cx.theme().clone();
        let h = if self.lay().touch() { TAP - 2. } else { 30. };
        h_flex().border_1().border_color(theme.input).rounded(theme.radius).overflow_hidden().children(items.into_iter().enumerate().map(|(i, (label, on, f))| {
            div()
                .id(SharedString::from(format!("{id}-{i}")))
                .h(px(h))
                .px_3()
                .flex()
                .items_center()
                .cursor_pointer()
                .when(on, |d| d.bg(theme.primary).text_color(theme.primary_foreground))
                .when(!on, |d| d.hover(|d| d.bg(theme.accent)))
                .child(label)
                .on_click(cx.listener(move |this, _, window, cx| {
                    f(this, window, cx);
                    cx.notify();
                }))
        }))
    }

    /// A pill that toggles a filter (the design's `.chipbtn`).
    pub fn chip_button(
        &self,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        on: bool,
        cx: &mut Context<Self>,
        f: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let theme = cx.theme().clone();
        let h = if self.lay().touch() { TAP } else { 36. };
        div()
            .id(id.into())
            .h(px(h))
            .px_3p5()
            .flex()
            .flex_none()
            .items_center()
            .gap_1p5()
            .rounded_full()
            .border_1()
            .border_color(if on { theme.primary } else { theme.input })
            .cursor_pointer()
            .when(on, |d| d.bg(theme.primary).text_color(theme.primary_foreground))
            .when(!on, |d| d.hover(|d| d.bg(theme.accent)))
            .child(label.into())
            .on_click(cx.listener(move |this, _, window, cx| {
                f(this, window, cx);
                cx.notify();
            }))
    }

    /// A tappable row that is not a button: a list item with a hover and a minimum height.
    pub fn row_button(&self, id: impl Into<SharedString>, cx: &Context<Self>) -> Stateful<Div> {
        let theme = cx.theme();
        let accent = theme.accent;
        h_flex().id(id.into()).w_full().min_h(px(if self.lay().touch() { 56. } else { 48. })).cursor_pointer().hover(move |d| d.bg(accent))
    }

    /// The scrim and the surface for a dialog: centred near the top on a laptop, a bottom sheet on
    /// touch layouts. `top` puts the surface at the top instead (the command palette); `full` makes
    /// it fill the window (the palette in portrait).
    pub fn overlay(&self, id: &'static str, top: bool, full: bool, content: impl IntoElement, close: fn(&mut Self), cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let touch = self.lay().touch();
        let scrim = div()
            .id(id)
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .flex()
            .justify_center()
            .bg(gpui_kit::hsla(0., 0., 0., if theme.is_dark() { 0.6 } else { 0.3 }))
            .map(|d| if full { d } else if top { d.items_start() } else if touch { d.items_end() } else { d.items_start().pt(px(110.)) })
            .on_click(cx.listener(move |this, _, _, cx| {
                close(this);
                cx.notify();
            }));
        let surface = v_flex()
            .id(SharedString::from(format!("{id}-surface")))
            .occlude()
            .gap_3()
            .bg(theme.popover)
            .border_1()
            .border_color(theme.border)
            .on_click(|_, _, cx| cx.stop_propagation())
            .map(|d| {
                if full {
                    d.size_full().p_4()
                } else if top {
                    d.w(px(if self.lay() == Layout::Laptop { 560. } else { 640. })).mt(px(if self.lay() == Layout::Laptop { 90. } else { 48. })).p_3().rounded(theme.radius_lg).shadow_lg()
                } else if touch {
                    d.w_full().max_w(px(720.)).max_h(relative(0.92)).overflow_y_scroll().p(px(24.)).pb(px(28.)).rounded_t(px(16.))
                } else {
                    d.w(px(560.)).max_h(relative(0.88)).overflow_y_scroll().p(px(24.)).rounded(theme.radius_lg).shadow_lg()
                }
            })
            .when(touch && !top, |d| d.child(div().w(px(40.)).h(px(4.)).rounded_full().bg(theme.border).self_center()))
            .child(content);
        scrim.child(surface).into_any_element()
    }
}

/// The colour theme the user chose: fixed, or following the system.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ThemePref {
    Dark,
    Light,
    Auto,
}

impl ThemePref {
    /// Kept in `<data dir>/theme`; dark unless the user chose otherwise.
    pub fn load() -> Self {
        match dtcdb::data_dir().and_then(|d| std::fs::read_to_string(d.join("theme")).ok()).as_deref().map(str::trim) {
            Some("light") => ThemePref::Light,
            Some("auto") => ThemePref::Auto,
            _ => ThemePref::Dark,
        }
    }

    pub fn save(self) {
        if let Some(dir) = dtcdb::data_dir() {
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(dir.join("theme"), match self {
                ThemePref::Dark => "dark",
                ThemePref::Light => "light",
                ThemePref::Auto => "auto",
            });
        }
    }

    pub fn is_dark(self, window: &Window) -> bool {
        match self {
            ThemePref::Dark => true,
            ThemePref::Light => false,
            ThemePref::Auto => matches!(window.appearance(), WindowAppearance::Dark | WindowAppearance::VibrantDark),
        }
    }
}

/// Applies the theme and the text size (the theme change resets the font size).
pub fn apply_theme(pref: ThemePref, window: &mut Window, cx: &mut App) {
    let dark = pref.is_dark(window);
    Theme::change(if dark { ThemeMode::Dark } else { ThemeMode::Light }, Some(window), cx);
    if !dark {
        // the kit's #737373 is 4.3:1 on the muted fills; the design darkens it to #666666 (5.7:1)
        Theme::global_mut(cx).colors.muted_foreground = gpui_kit::rgb(0x666666).into();
    }
    set_font_size(tablet(), window, cx);
}

/// Everything the new layout needs to remember between frames.
pub struct UiState {
    pub layout: Layout,
    pub theme: ThemePref,
    pub report_menu: bool,
    pub more: bool,
    pub settings: bool,
    /// The fault code open on the Fault codes page, and whether the detail (not the list) is showing.
    pub sel: Option<String>,
    pub detail: bool,
    /// A live signal picked out on the Live data page.
    pub hi_sig: Option<String>,
    /// Signals in the combined chart (`None`: the first three cards), the scrub position in seconds
    /// into the last minute, and bench mode.
    pub charted: Option<Vec<String>>,
    pub scrub: f32,
    pub scrub_state: Entity<SliderState>,
    pub bench: bool,
    /// Functions page filters.
    pub fn_only_run: bool,
    pub fn_need_run: bool,
    /// Procedures: the job is open (not the list), and the tick on its current step.
    pub proc_open: bool,
    pub proc_tick: bool,
    /// Trace: only sent (`Some(true)`) or only received lines.
    pub trace_dir: Option<bool>,
}

impl UiState {
    pub fn new(scrub_state: Entity<SliderState>) -> Self {
        Self { layout: Layout::Laptop, theme: ThemePref::load(), report_menu: false, more: false, settings: false, sel: None, detail: false, hi_sig: None, charted: None, scrub: 60., scrub_state, bench: false, fn_only_run: false, fn_need_run: false, proc_open: false, proc_tick: false, trace_dir: None }
    }
}
