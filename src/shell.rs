//! The frame around every page: navigation (sidebar, icon rail or bottom tab bar, by window width),
//! the header with search and the Report menu, the status strip, and the overlays that belong to
//! the frame (Settings, the More sheet, the report menu's click-away).
//!
//! This is a child module of `main`, so it can use the `Diagnostics` view's state directly.

use super::*;

use crate::i18n::tr;
use crate::ui::{Layout, r, TAP};

impl Diagnostics {
    /// Opens a page and closes whatever was open over the old one.
    pub fn go(&mut self, page: Page, cx: &mut Context<Self>) {
        self.page = page;
        self.ui.more = false;
        self.ui.settings = false;
        self.ui.report_menu = false;
        self.ws.palette_open = false;
        self.ensure_modules();
        self.ensure_vin();
        cx.notify();
    }

    fn nav_item(&self, page: Page, count: usize, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = cx.theme().clone();
        let on = self.page == page;
        let id = SharedString::from(format!("nav-{}", page.label()));
        let badge = page == Page::Codes && count > 0;
        let pill = |theme: &gpui_kit::component::Theme| {
            div().min_w(px(20.)).h(px(20.)).px_1p5().rounded_full().bg(theme.primary).text_color(theme.primary_foreground).text_size(r(12.)).text_center().child(count.to_string())
        };
        let base = div().id(id).relative().rounded(theme.radius).cursor_pointer().text_color(theme.sidebar_foreground);
        let base = if on { base.bg(theme.sidebar_accent).font_weight(FontWeight::SEMIBOLD) } else { base.hover(|d| d.bg(theme.sidebar_accent)) };
        let base = base.on_click(cx.listener(move |this, _, _, cx| this.go(page, cx)));
        match self.lay() {
            Layout::Tablet => base
                .h(px(58.))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_0p5()
                .px_0p5()
                .text_size(r(11.))
                .text_center()
                .child(Icon::new(page.icon()).size(px(20.)))
                .child(page.label())
                .when(badge, |d| d.child(pill(&theme).absolute().top(px(3.)).right(px(8.)))),
            _ => base
                .h(px(if self.lay().touch() { TAP } else { 36. }))
                .px_2()
                .flex()
                .items_center()
                .gap_2()
                .child(Icon::new(page.icon()).size(px(18.)))
                .child(page.label())
                .when(badge, |d| d.child(div().ml_auto().child(pill(&theme)))),
        }
    }

    /// The sidebar (laptop) or the icon rail (tablet landscape).
    fn render_nav(&mut self, count: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let rail = self.lay() == Layout::Tablet;
        let vehicle_title = match self.current.and_then(|ix| self.vehicles.get(ix)) {
            Some(v) => v.name.clone(),
            None => self.make.clone().unwrap_or_else(|| tr("Select a vehicle").into()),
        };
        let vehicle_vin = self.vin.as_deref().map_or(tr("VIN not read").to_string(), health::mask_vin);
        let groups = Page::groups().into_iter().enumerate().map(|(gi, (heading, pages))| {
            v_flex()
                .gap_0p5()
                .child(if rail {
                    div().h(px(if gi == 0 { 0. } else { 1. })).mx_2().my_1p5().bg(theme.border)
                } else {
                    div().px_2().pt_2().text_size(r(11.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(tr(heading).to_uppercase())
                })
                .children(pages.into_iter().map(|p| self.nav_item(p, count, cx)).collect::<Vec<_>>())
        }).collect::<Vec<_>>();
        let settings = div()
            .id("nav-settings")
            .rounded(theme.radius)
            .cursor_pointer()
            .hover(|d| d.bg(theme.sidebar_accent))
            .on_click(cx.listener(|this, _, _, cx| {
                this.ui.settings = true;
                this.ui.more = false;
                cx.notify();
            }))
            .map(|d| {
                if rail {
                    d.h(px(58.)).flex().flex_col().items_center().justify_center().gap_0p5().text_size(r(11.))
                } else {
                    d.h(px(if self.lay().touch() { TAP } else { 32. })).px_2().flex().items_center().gap_2()
                }
            })
            .child(Icon::new(IconName::Settings).size(px(if rail { 20. } else { 18. })))
            .child(tr("Settings"));
        v_flex()
            .w(px(if rail { 92. } else { 208. }))
            .flex_none()
            .h_full()
            .gap(px(if rail { 8. } else { 16. }))
            .p(px(if rail { 8. } else { 12. }))
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.sidebar_border)
            .when(!rail, |d| {
                d.child(
                    h_flex()
                        .id("vehicle-card")
                        .gap_2()
                        .p_2()
                        .items_center()
                        .justify_between()
                        .border_1()
                        .border_color(theme.border)
                        .rounded(theme.radius)
                        .cursor_pointer()
                        .when(self.page == Page::Vehicles, |d| d.bg(theme.sidebar_accent))
                        .hover(|d| d.bg(theme.sidebar_accent))
                        .on_click(cx.listener(|this, _, _, cx| this.go(Page::Vehicles, cx)))
                        .child(
                            v_flex()
                                .min_w_0()
                                .child(div().font_weight(FontWeight::SEMIBOLD).truncate().child(vehicle_title))
                                .child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(vehicle_vin)),
                        )
                        .child(Icon::new(IconName::ChevronsUpDown).size_4().text_color(theme.muted_foreground)),
                )
            })
            .child(v_flex().id("nav-list").flex_1().min_h_0().overflow_y_scroll().gap_2().children(groups))
            .child(v_flex().child(settings))
            .into_any_element()
    }

    /// The bottom tab bar of the portrait layout.
    fn render_tabbar(&self, count: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let more_pages = [Page::Modules, Page::History, Page::Functions, Page::Procedures, Page::Trace, Page::Contribute, Page::Vehicles];
        let tab = |id: &'static str, icon: IconName, label: &'static str, on: bool, badge: usize, cx: &mut Context<Self>, f: fn(&mut Self, &mut Context<Self>)| {
            div()
                .id(id)
                .relative()
                .flex_1()
                .min_h(px(56.))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_0p5()
                .rounded(px(8.))
                .cursor_pointer()
                .text_size(r(11.))
                .when(on, |d| d.bg(theme.sidebar_accent).font_weight(FontWeight::SEMIBOLD))
                .when(!on, |d| d.hover(|d| d.bg(theme.sidebar_accent)))
                .child(Icon::new(icon).size(px(22.)))
                .child(label)
                .when(badge > 0, |d| {
                    d.child(
                        div()
                            .absolute()
                            .top(px(2.))
                            .left(relative(0.5))
                            .ml(px(6.))
                            .min_w(px(18.))
                            .h(px(18.))
                            .px_1p5()
                            .rounded_full()
                            .bg(theme.primary)
                            .text_color(theme.primary_foreground)
                            .text_size(r(11.))
                            .text_center()
                            .child(badge.to_string()),
                    )
                })
                .on_click(cx.listener(move |this, _, _, cx| f(this, cx)))
        };
        h_flex()
            .flex_none()
            .gap_1()
            .px_2()
            .py_1p5()
            .bg(theme.sidebar)
            .border_t_1()
            .border_color(theme.border)
            .child(tab("tab-overview", Page::Overview.icon(), Page::Overview.label(), self.page == Page::Overview, 0, cx, |t, cx| t.go(Page::Overview, cx)))
            .child(tab("tab-codes", Page::Codes.icon(), Page::Codes.label(), self.page == Page::Codes, count, cx, |t, cx| t.go(Page::Codes, cx)))
            .child(tab("tab-live", Page::Live.icon(), Page::Live.label(), self.page == Page::Live, 0, cx, |t, cx| t.go(Page::Live, cx)))
            .child(tab("tab-readiness", Page::Readiness.icon(), Page::Readiness.label(), self.page == Page::Readiness, 0, cx, |t, cx| t.go(Page::Readiness, cx)))
            .child(tab("tab-more", IconName::Ellipsis, tr("More"), more_pages.contains(&self.page), 0, cx, |t, cx| {
                t.ui.more = true;
                cx.notify();
            }))
            .into_any_element()
    }

    fn render_search_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let portrait = self.lay() == Layout::Portrait;
        div()
            .id("search")
            .h(self.lay().button())
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .border_1()
            .border_color(theme.input)
            .rounded(theme.radius)
            .bg(theme.background)
            .text_color(theme.muted_foreground)
            .cursor_pointer()
            .hover(|d| d.bg(theme.accent))
            .map(|d| if portrait { d.w(px(TAP)).justify_center().px_0() } else { d.min_w(px(220.)) })
            .child(Icon::new(IconName::Search).size_4())
            .when(!portrait, |d| d.child(div().flex_1().child(tr("Search…"))))
            .when(self.lay() == Layout::Laptop, |d| {
                d.child(div().h(px(18.)).px_1p5().flex().items_center().border_1().border_color(theme.border).rounded(px(3.)).text_size(r(11.)).child("Ctrl K"))
            })
            .on_click(cx.listener(|this, _, window, cx| this.open_palette(window, cx)))
            .into_any_element()
    }

    /// The Report dropdown: one button instead of three. The menu is drawn deferred so it sits above
    /// the page, and `render_ui` adds the click-away layer under it.
    fn render_report_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let touch = self.lay().touch();
        let item = |id: &'static str, label: &'static str, cx: &mut Context<Self>, f: fn(&mut Self, &mut Context<Self>)| {
            div()
                .id(id)
                .min_h(px(if touch { 48. } else { 40. }))
                .px_2p5()
                .flex()
                .items_center()
                .gap_2()
                .rounded(theme.radius)
                .cursor_pointer()
                .hover(|d| d.bg(theme.accent))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.ui.report_menu = false;
                    f(this, cx);
                    cx.notify();
                }))
        };
        div()
            .relative()
            .child(
                Button::new("report")
                    .h(self.lay().button())
                    .child(h_flex().gap_2().items_center().child(tr("Report")).child(Icon::new(IconName::ChevronDown).size_4()))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.ui.report_menu = !this.ui.report_menu;
                        cx.notify();
                    })),
            )
            .when(self.ui.report_menu, |d| {
                d.child(
                    deferred(
                        v_flex()
                            .absolute()
                            .top_full()
                            .right_0()
                            .mt_1()
                            .min_w(px(240.))
                            .p_1()
                            .occlude()
                            .bg(theme.popover)
                            .border_1()
                            .border_color(theme.border)
                            .rounded(theme.radius_lg)
                            .shadow_lg()
                            .child(item("rep-tech", tr("Technical PDF"), cx, |t, cx| t.save_pdf(false, cx)))
                            .child(item("rep-cust", tr("Customer PDF"), cx, |t, cx| t.save_pdf(true, cx)))
                            .child(item("rep-data", tr("Save report data"), cx, |t, cx| t.save_report(cx)))
                            .child(item("rep-copy", tr("Copy summary"), cx, |t, cx| {
                                let text = report::text(&t.build_report(cx));
                                t.copy_text(text, "the vehicle and its codes", cx);
                            })),
                    )
                    .with_priority(2),
                )
            })
            .into_any_element()
    }

    /// Title, subtitle, search, the Report menu and the page's own actions.
    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let online = matches!(self.link, Link::Online { .. });
        let engine_on = self.engine_running() == Some(true);
        let touch = self.lay().touch();
        let h = self.lay().button();
        let has_report = matches!(self.page, Page::Overview | Page::Codes | Page::History | Page::Readiness);
        let actions: Vec<AnyElement> = match self.page {
            Page::Overview => vec![Button::new("scan-all")
                .h(h)
                .primary()
                .label(tr(if self.scanning { "Scanning…" } else { "Scan all modules" }))
                .disabled(!online || self.scanning)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.start_read_codes();
                    this.start_scan();
                    cx.notify();
                }))
                .into_any_element()],
            Page::Codes => vec![
                Button::new("rescan")
                    .h(h)
                    .label(tr(if self.reading_codes { "Reading…" } else { "Read codes" }))
                    .disabled(!online || self.reading_codes)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.start_read_codes();
                        this.start_scan();
                        cx.notify();
                    }))
                    .into_any_element(),
                div()
                    .ml_4()
                    .child(
                        Button::new("clear")
                            .h(h)
                            .outline()
                            .danger()
                            .label(tr(if engine_on { "Stop the engine to clear" } else { "Clear codes…" }))
                            .disabled(self.code_count(cx) == 0 || !online || engine_on || self.confirm_clear)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.confirm_clear = true;
                                this.ws.clear_result = None;
                                cx.notify();
                            })),
                    )
                    .into_any_element(),
            ],
            Page::Modules => vec![Button::new("scan-modules")
                .h(h)
                .primary()
                .label(tr(if self.scanning { "Scanning…" } else { "Scan modules" }))
                .disabled(!online || self.scanning)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.start_scan();
                    cx.notify();
                }))
                .into_any_element()],
            Page::Contribute => vec![Button::new("export")
                .h(h)
                .label(format!("Export {} contribution(s)", self.contrib.len()))
                .on_click(cx.listener(|this, _, _, cx| this.export_contributions(cx)))
                .into_any_element()],
            Page::Vehicles => vec![Button::new("read-vin")
                .h(h)
                .label("Read VIN from vehicle")
                .disabled(!online)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.notice = Some("Reading the VIN…".into());
                    let _ = this.backend.cmds.send(Cmd::ReadVin);
                    cx.notify();
                }))
                .into_any_element()],
            _ => Vec::new(),
        };
        h_flex()
            .flex_none()
            .flex_wrap()
            .items_start()
            .gap_3()
            .px(px(self.lay().gutter()))
            .pt(px(if touch { 16. } else { 20. }))
            .child(
                v_flex()
                    .gap_0p5()
                    .child(div().text_size(r(20.)).line_height(r(28.)).font_weight(FontWeight::SEMIBOLD).child(self.page.label()))
                    .child(div().text_color(theme.muted_foreground).child(self.page_subtitle(cx))),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .items_center()
                    .ml_auto()
                    .child(self.render_search_button(cx))
                    .when(has_report, |d| d.child(self.render_report_menu(cx)))
                    .children(actions),
            )
            .into_any_element()
    }

    /// Check-engine light, vehicle, battery, engine, adapter, and the last message. At the bottom on
    /// a laptop; at the top in the touch layouts, where the title bar's light moves in here.
    fn render_status_strip(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let layout = self.lay();
        let touch = layout.touch();
        let portrait = layout == Layout::Portrait;
        let dot = |c: Hsla| div().size(px(8.)).rounded_full().bg(c).flex_none();
        let chip = || h_flex().gap_1p5().items_center().flex_none();
        let (link_text, link_color) = match &self.link {
            Link::Connecting => (tr("Connecting…").to_string(), theme.muted_foreground),
            Link::Online { adapter, .. } => (format!("{adapter} · {}", tr("connected")), theme.green),
            Link::Offline(_) => (format!("{} · {}", tr("Adapter offline"), tr("Retrying…")), theme.red),
        };
        let battery = self.readings.get("pid:42").copied().map(|v| (v, health::battery(v, self.rpm())));
        let engine = match self.engine_running() {
            Some(true) => tr("Engine running"),
            Some(false) => tr("Engine off"),
            None => tr("Engine speed not reported"),
        };
        let vehicle = match self.current.and_then(|ix| self.vehicles.get(ix)) {
            Some(v) => v.name.clone(),
            None => self.make.clone().unwrap_or_default(),
        };
        h_flex()
            .flex_none()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap_x_4()
            .gap_y_1()
            .px(px(if touch { 20. } else { 12. }))
            .py_1()
            .min_h(px(if touch { 48. } else { 36. }))
            .bg(theme.title_bar)
            .map(|d| if touch { d.border_b_1() } else { d.border_t_1() })
            .border_color(theme.border)
            .text_size(r(12.))
            .child(
                h_flex()
                    .flex_wrap()
                    .items_center()
                    .gap_x_4()
                    .gap_y_1()
                    .when(touch, |d| {
                        d.child(
                            chip()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(if self.mil_on { theme.red } else { theme.green })
                                .child(dot(if self.mil_on { theme.red } else { theme.green }))
                                .child(tr(if self.mil_on { "Check-engine light ON" } else { "Check-engine light off" })),
                        )
                    })
                    .when(touch && !portrait && !vehicle.is_empty(), |d| d.child(chip().font_weight(FontWeight::SEMIBOLD).child(vehicle.clone())))
                    .child(match battery {
                        Some((v, (text, level))) => chip()
                            .child(Icon::new(IconName::BatteryMedium).size_4().text_color(level_color(level, &theme)))
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(format!("{} V", units::localise(format!("{v:.1}")))))
                            .when(!portrait, |d| d.child(div().text_color(level_color(level, &theme)).child(format!("· {text}")))),
                        None => chip().child(Icon::new(IconName::BatteryMedium).size_4().text_color(theme.muted_foreground)).child(div().text_color(theme.muted_foreground).child(tr("Battery not reported"))),
                    })
                    .child(chip().child(engine))
                    .child(chip().text_color(link_color).child(dot(link_color)).child(link_text)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .min_w_0()
                    .children(self.toast.clone().map(|t| div().text_color(theme.foreground).truncate().max_w(px(if portrait { 360. } else { 640. })).child(t)))
                    .when(!portrait, |d| d.children(self.vin.as_deref().map(|v| div().text_color(theme.muted_foreground).child(health::mask_vin(v)))))
                    .child(
                        Button::new("strip-read")
                            .ghost()
                            .h(px(if touch { TAP } else { 28. }))
                            .icon(IconName::RefreshCw)
                            .label(tr(if self.reading_codes { "Reading…" } else { "Read codes" }))
                            .disabled(!matches!(self.link, Link::Online { .. }) || self.reading_codes)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.start_read_codes();
                                cx.notify();
                            })),
                    )
                    .children(self.vin.clone().map(|vin| {
                        Button::new("copy-vin")
                            .ghost()
                            .h(px(if touch { TAP } else { 28. }))
                            .icon(IconName::Copy)
                            .label(tr("Copy VIN"))
                            .on_click(cx.listener(move |this, _, _, cx| this.copy_text(vin.clone(), "the VIN", cx)))
                    })),
            )
            .into_any_element()
    }

    /// Language, theme and text size.
    fn render_settings(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.ui.settings {
            return None;
        }
        let theme = cx.theme().clone();
        let pref = self.ui.theme;
        let pt = i18n::is_pt();
        let setrow = |label: &'static str, control: Div| h_flex().min_h(px(48.)).justify_between().items_center().gap_4().flex_wrap().child(label).child(control);
        let content = v_flex()
            .gap_3()
            .child(div().text_size(r(20.)).line_height(r(28.)).font_weight(FontWeight::SEMIBOLD).child(tr("Settings")))
            .child(setrow(
                tr("Language"),
                self.seg("set-lang", [("English", !pt, |_, _, _| i18n::set_pt(false)), ("Português", pt, |_, _, _| i18n::set_pt(true))], cx),
            ))
            .child(setrow(
                tr("Theme"),
                self.seg(
                    "set-theme",
                    [
                        (tr("Dark"), pref == ui::ThemePref::Dark, |t, w, cx| t.set_theme(ui::ThemePref::Dark, w, cx)),
                        (tr("Light"), pref == ui::ThemePref::Light, |t, w, cx| t.set_theme(ui::ThemePref::Light, w, cx)),
                        (tr("Auto"), pref == ui::ThemePref::Auto, |t, w, cx| t.set_theme(ui::ThemePref::Auto, w, cx)),
                    ],
                    cx,
                ),
            ))
            .child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(tr("Auto follows the device. Use Light in bright workshops or outdoors.")))
            .child(setrow(
                tr("Text size"),
                self.seg(
                    "set-size",
                    [
                        (tr("Normal"), !tablet(), |_, w, cx| apply_size(false, w, cx)),
                        (tr("Larger"), tablet(), |_, w, cx| apply_size(true, w, cx)),
                    ],
                    cx,
                ),
            ))
            .child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(tr("Larger text is for touch use at the car. A narrow window uses the touch layout by itself.")))
            .child(setrow(tr("Temperature"), self.seg("set-temp", [("°C", !units::fahrenheit(), |_, _, _| units::set_fahrenheit(false)), ("°F", units::fahrenheit(), |_, _, _| units::set_fahrenheit(true))], cx)))
            .child(setrow(tr("Pressure"), self.seg("set-press", [("kPa", !units::psi(), |_, _, _| units::set_psi(false)), ("psi", units::psi(), |_, _, _| units::set_psi(true))], cx)))
            .child(setrow(tr("Speed"), self.seg("set-speed", [("km/h", !units::mph(), |_, _, _| units::set_mph(false)), ("mph", units::mph(), |_, _, _| units::set_mph(true))], cx)))
            .child(setrow(tr("Shop name"), div().w(px(280.)).child(Input::new(&self.ws.shop_input).when(self.lay().touch(), |i| i.h(px(TAP))))))
            .child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(tr("The shop name is printed at the top of reports.")))
            .child(ui::verdict_at(ui::Tone::Ok, tr("Works offline. Everything stays on this device; nothing is sent anywhere."), 16., 12., &theme))
            .child(h_flex().justify_end().child(self.sized(Button::new("settings-done").primary().label(tr("Done"))).on_click(cx.listener(|this, _, _, cx| {
                this.ui.settings = false;
                cx.notify();
            }))));
        Some(self.overlay("settings", false, false, content, |t| t.ui.settings = false, cx))
    }

    pub fn set_theme(&mut self, pref: ui::ThemePref, window: &mut Window, cx: &mut Context<Self>) {
        self.ui.theme = pref;
        pref.save();
        ui::apply_theme(pref, window, cx);
    }

    /// The portrait layout's More sheet: every page the tab bar has no room for.
    fn render_more(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.ui.more {
            return None;
        }
        let theme = cx.theme().clone();
        let pages = [Page::Modules, Page::History, Page::Functions, Page::Procedures, Page::Trace, Page::Contribute, Page::Vehicles];
        let content = v_flex().gap_1().children(pages.into_iter().map(|p| {
            self.row_button(format!("more-{}", p.label()), cx)
                .gap_3()
                .px_3()
                .rounded(px(8.))
                .when(self.page == p, |d| d.bg(theme.sidebar_accent).font_weight(FontWeight::SEMIBOLD))
                .child(div().flex_1().child(p.label()))
                .child(Icon::new(IconName::ChevronRight).size_4().text_color(theme.muted_foreground))
                .on_click(cx.listener(move |this, _, _, cx| this.go(p, cx)))
        }).chain(std::iter::once(
            self.row_button("more-settings", cx)
                .gap_3()
                .px_3()
                .rounded(px(8.))
                .child(div().flex_1().child(tr("Settings")))
                .child(Icon::new(IconName::ChevronRight).size_4().text_color(theme.muted_foreground))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.ui.more = false;
                    this.ui.settings = true;
                    cx.notify();
                })),
        )));
        Some(self.overlay("more", false, false, content, |t| t.ui.more = false, cx))
    }

    /// A procedure is under way and the mechanic is somewhere else: where it is, and a way back.
    fn render_procedure_bar(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let run = self.ws.proc.as_ref()?;
        let p = &procedures::ALL[run.index];
        if run.step == 0 || run.step >= p.steps.len() || (self.page == Page::Procedures && self.ui.proc_open) {
            return None;
        }
        let theme = cx.theme().clone();
        let text = tr("Procedure in progress: {name}, step {n} of {m}").replace("{name}", tr(p.title)).replace("{n}", &(run.step + 1).to_string()).replace("{m}", &p.steps.len().to_string());
        Some(
            h_flex()
                .flex_none()
                .flex_wrap()
                .gap_3()
                .mx(px(self.lay().gutter()))
                .mt_3()
                .px_4()
                .py_2p5()
                .items_center()
                .border_1()
                .border_color(theme.yellow)
                .rounded(theme.radius)
                .child(ui::clock_icon().size(px(20.)).text_color(theme.yellow))
                .child(div().flex_1().min_w(px(200.)).font_weight(FontWeight::SEMIBOLD).child(text))
                .child(self.sized(Button::new("proc-return").primary().label(tr("Return to it"))).on_click(cx.listener(|this, _, _, cx| {
                    this.ui.proc_open = true;
                    this.go(Page::Procedures, cx);
                })))
                .child(self.sized(Button::new("proc-abort").label(tr("Stop it"))).on_click(cx.listener(|this, _, _, cx| {
                    this.ws.proc = None;
                    this.ui.proc_open = false;
                    this.toast = Some(tr("Procedure stopped. Anything already started in a module keeps running on its own.").into());
                    cx.notify();
                })))
                .into_any_element(),
        )
    }

    /// While something is being written to a module, a low or unsteady supply is shown on every page.
    fn render_power_bar(&self, cx: &App) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let writing = self.confirm_run.is_some() || self.ws.proc.as_ref().is_some_and(|r| r.step > 0 && r.step < procedures::ALL[r.index].steps.len());
        if !writing {
            return None;
        }
        let (tone, text) = match self.write_gate() {
            health::Gate::Ok(_) => return None,
            health::Gate::Warn(t) => (ui::Tone::Warn, t.to_string()),
            health::Gate::Block(t) => (ui::Tone::Bad, t.to_string()),
        };
        Some(ui::verdict_at(tone, text, 22., 16., &theme).mx(px(self.lay().gutter())).mt_3().px_4().py_3().border_1().border_color(tone.color(&theme)).rounded(px(8.)).into_any_element())
    }

    /// The whole window.
    pub fn render_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.page.dev_save();
        self.ui.layout = Layout::detect(f32::from(window.viewport_size().width));
        let layout = self.lay();
        let touch = layout.touch();
        if self.page == Page::Codes {
            self.sync_code_note(window, cx);
        }
        let count = self.faults(cx).len();
        let ask_make = matches!(self.link, Link::Online { .. }) && self.make.is_none();
        let theme = cx.theme().clone();

        let titlebar = TitleBar::new().child(
            h_flex()
                .w_full()
                .justify_between()
                .items_center()
                .child(div().text_xs().text_color(theme.muted_foreground).child(tr("Diagnostics")))
                .when(!touch, |d| {
                    d.child(
                        div()
                            .mr_4()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if self.mil_on { theme.red } else { theme.green })
                            .child(format!("● {}", tr(if self.mil_on { "Check-engine light ON" } else { "Check-engine light off" }))),
                    )
                }),
        );

        let offline = match &self.link {
            Link::Offline(why) => Some(
                h_flex()
                    .flex_none()
                    .gap_3()
                    .mx(px(layout.gutter()))
                    .mt_3()
                    .px_4()
                    .py_3()
                    .items_center()
                    .border_1()
                    .border_color(theme.red)
                    .rounded(theme.radius)
                    .child(Icon::new(IconName::CircleX).size(px(20.)).text_color(theme.red))
                    .child(
                        v_flex()
                            .flex_1()
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(tr("Connection to the car lost")))
                            .child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(format!(
                                "{why}. {}",
                                tr("Reconnecting every 3 s. Nothing is sent to the car while it is offline, and a procedure resumes at the step it stopped on.")
                            ))),
                    )
                    .child(self.sized(Button::new("banner-reconnect").label(tr("Reconnect"))).on_click(cx.listener(|this, _, _, cx| {
                        let _ = this.backend.cmds.send(Cmd::Reconnect);
                        this.toast = Some("Reconnecting to the adapter…".into());
                        cx.notify();
                    })))
                    .into_any_element(),
            ),
            _ => None,
        };

        let make_banner = ask_make.then(|| {
            h_flex()
                .flex_none()
                .gap_3()
                .px_3()
                .py_2()
                .items_center()
                .bg(theme.yellow.opacity(0.15))
                .border_b_1()
                .border_color(theme.border)
                .child(div().flex_1().text_base().text_color(theme.foreground).child("Vehicle not recognised. Pick or add it so manufacturer-specific codes can be named."))
                .child(self.sized(Button::new("pick-vehicle").label(tr("Choose vehicle"))).on_click(cx.listener(|this, _, _, cx| this.go(Page::Vehicles, cx))))
                .into_any_element()
        });

        let proc_bar = self.render_procedure_bar(cx);
        let power_bar = self.render_power_bar(cx);
        let page = match self.page {
            Page::Overview => self.render_overview(cx).into_any_element(),
            Page::Codes => self.render_codes(cx).into_any_element(),
            Page::Live => self.render_dashboard(cx).into_any_element(),
            Page::Readiness => self.render_readiness_page(cx).into_any_element(),
            Page::Modules => self.render_modules(cx).into_any_element(),
            Page::Functions => self.render_functions(window, cx).into_any_element(),
            Page::Contribute => self.render_contribute(window, cx).into_any_element(),
            Page::Vehicles => self.render_vehicles(cx).into_any_element(),
            Page::Procedures => self.render_procedures(cx).into_any_element(),
            Page::History => self.render_history(cx).into_any_element(),
            Page::Trace => self.render_trace(cx).into_any_element(),
        };
        let nav = (layout != Layout::Portrait).then(|| self.render_nav(count, cx));
        let header = self.render_header(cx);
        let strip = self.render_status_strip(cx);
        let (top_strip, bottom_strip) = if touch { (Some(strip), None) } else { (None, Some(strip)) };
        let tabbar = (layout == Layout::Portrait).then(|| self.render_tabbar(count, cx));
        let run_modal = self.run_modal(cx);
        let picker = self.picker_modal(window, cx);
        let palette = self.palette_overlay(cx);
        let settings = self.render_settings(cx);
        let more = self.render_more(cx);

        v_flex()
            .size_full()
            .key_context("canforge")
            .on_action(cx.listener(|this, _: &OpenPalette, window, cx| this.open_palette(window, cx)))
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(titlebar)
            .children(top_strip)
            .children(make_banner)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .children(nav)
                    .child(v_flex().flex_1().min_w_0().h_full().child(header).children(offline).children(power_bar).children(proc_bar).child(div().flex_1().min_h_0().overflow_hidden().child(page))),
            )
            .children(tabbar)
            .children(bottom_strip)
            .when(self.ui.report_menu, |d| {
                d.child(
                    deferred(div().id("report-away").absolute().top_0().left_0().size_full().on_click(cx.listener(|this, _, _, cx| {
                        this.ui.report_menu = false;
                        cx.notify();
                    })))
                    .with_priority(1),
                )
            })
            .children(run_modal)
            .children(picker)
            .children(settings)
            .children(more)
            .children(palette)
            .into_any_element()
    }
}
