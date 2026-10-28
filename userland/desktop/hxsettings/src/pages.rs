use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hxstd::permissions;
use hxstd::{fs, sys, users};
use hxtoolkit::{button, column, divider, dropdown, icon, input, list, picture, row, slider, switch, text, Align, Element, Length, Stack, TextSize, Tone};

use crate::system;
use crate::{hostname, Msg, Page, Prefs, Sub, DOCK_LIMITS, DOCK_SIZES, EFFECTS, PAGES, SPEEDS};

const NAV_ROW: u32 = 58;

fn group(title: &str, rows: Vec<Element<Msg>>) -> Element<Msg> {
    let mut card = column![].spacing(0).padding([4, 18]).card();
    for (i, r) in rows.into_iter().enumerate() {
        if i > 0 {
            card = card.push(divider());
        }
        card = card.push(column![r].padding([12, 0]));
    }
    if title.is_empty() {
        return card.into();
    }
    column![text(title).strong(), card].spacing(10).into()
}

fn item(title: &str, subtitle: &str, control: impl Into<Element<Msg>>) -> Element<Msg> {
    let mut info = column![text(title)].spacing(3).width(Length::Fill);
    if !subtitle.is_empty() {
        info = info.push(text(subtitle).small().tone(Tone::Dim).wrap());
    }
    row![info, control.into()].spacing(18).align(Align::Center).into()
}

fn value(title: &str, value: impl Into<String>) -> Element<Msg> {
    row![text(title).width(Length::Fill), text(value.into()).tone(Tone::Dim).selectable()].spacing(18).align(Align::Center).into()
}

fn note(content: impl Into<String>) -> Element<Msg> {
    text(content.into()).small().tone(Tone::Dim).wrap().into()
}

fn nav(icon_name: &str, title: &str, subtitle: &str) -> Element<Msg> {
    let mut info = column![text(title)].spacing(3).width(Length::Fill);
    if !subtitle.is_empty() {
        info = info.push(text(subtitle).small().tone(Tone::Dim));
    }
    let mut r = row![].spacing(14).align(Align::Center);
    if !icon_name.is_empty() {
        r = r.push(icon(icon_name).size(28u32, 28u32));
    }
    r.push(info).push(icon("ui/chevron-right").size(16u32, 16u32).tone(Tone::Faint)).into()
}

fn nav_group(title: &str, rows: Vec<Element<Msg>>, on: fn(usize) -> Msg) -> Element<Msg> {
    let count = rows.len();
    let l = list(rows).selected(None).on_select(on).row_height(NAV_ROW).height(count.max(1) as u32 * (NAV_ROW + 2) + 8);
    let card = column![l].padding([2, 2]).card();
    if title.is_empty() {
        return card.into();
    }
    column![text(title).strong(), card].spacing(10).into()
}

fn page(children: Vec<Element<Msg>>) -> Stack<Msg> {
    hxtoolkit::column(children).spacing(28).width(Length::Fill)
}

fn privilege_hint() -> Element<Msg> {
    let hint = if system::privileged_without_password() { "No password needed: Settings may change system settings" } else { "Applying asks for an administrator password" };
    row![icon("ui/lock").size(14u32, 14u32).tone(Tone::Faint), text(hint).small().tone(Tone::Faint)].spacing(8).align(Align::Center).width(Length::Fill).into()
}

pub fn view(p: &Prefs) -> Element<Msg> {
    let (title, back) = match (p.page, p.sub) {
        (Page::Apps, Sub::App(i)) => (p.apps.get(i).map(|a| a.name.clone()).unwrap_or_default(), true),
        (_, Sub::About) => (String::from("About this computer"), true),
        (_, Sub::Startup) => (String::from("Startup & Linux programs"), true),
        (_, Sub::Boot) => (String::from("Boot status"), true),
        _ => (String::from(PAGES.iter().find(|x| x.0 == p.page).map(|x| x.2).unwrap_or("")), false),
    };
    let mut header = row![].spacing(10).align(Align::Center).padding([18, 40, 6, 40]);
    if back {
        header = header.push(button("").icon("ui/chevron-left").flat().tooltip("Back").on_press(Msg::Back));
    }
    header = header.push(text(title).size(TextSize::Title));
    let content = match (p.page, p.sub) {
        (Page::Network, _) => network(p),
        (Page::Display, _) => display(p),
        (Page::Sound, _) => sound(p),
        (Page::Appearance, _) => appearance(p),
        (Page::Desktop, _) => desktop(p),
        (Page::Apps, Sub::App(i)) => app_detail(p, i),
        (Page::Apps, _) => apps(p),
        (Page::Users, _) => users_page(p),
        (Page::System, Sub::About) => about(),
        (Page::System, Sub::Startup) => startup(p),
        (Page::System, Sub::Boot) => boot(),
        (Page::System, _) => system_page(),
    };
    let mut main = column![header, hxtoolkit::scroll(content.padding([16, 40, 32, 40])).height(Length::Fill).width(Length::Fill)].spacing(0).width(Length::Fill).height(Length::Fill);
    if let Some(bar) = unlock_bar(p) {
        main = main.push(column![bar].padding([0, 40, 16, 40]));
    } else if !p.status.0.is_empty() {
        main = main.push(column![divider(), column![text(p.status.0.clone()).small().tone(p.status.1)].padding([10, 40])].spacing(0));
    }
    row![sidebar(p), main].spacing(0).align(Align::Stretch).width(Length::Fill).height(Length::Fill).into()
}

fn sidebar(p: &Prefs) -> Element<Msg> {
    let rows: Vec<Element<Msg>> = PAGES.iter().map(|x| row![icon(x.3).size(18u32, 18u32), text(x.2)].spacing(12).align(Align::Center).into()).collect();
    let selected = PAGES.iter().position(|x| x.0 == p.page);
    column![
        row![icon("apps/settings").size(28u32, 28u32), text("Settings").strong()].spacing(12).align(Align::Center).padding([4, 8]),
        list(rows).selected(selected).on_select(Msg::Page).row_height(38).height(Length::Fill),
        row![icon("ui/user").size(16u32, 16u32).tone(Tone::Dim), text(format!("{} @ {}", system::user(), hostname())).small().tone(Tone::Dim)].spacing(8).align(Align::Center).padding([4, 8]),
    ]
    .spacing(16)
    .padding([18, 10])
    .sidebar()
    .width(224u32)
    .height(Length::Fill)
    .into()
}

fn unlock_bar(p: &Prefs) -> Option<Element<Msg>> {
    let error = p.unlock.as_ref()?;
    let about = if error.is_empty() { String::from("This change affects the whole computer. Enter the password of an administrator to continue.") } else { error.clone() };
    Some(
        column![
            row![icon("ui/lock").size(22u32, 22u32), column![text("Administrator password required").strong(), text(about).small().tone(if error.is_empty() { Tone::Dim } else { Tone::Danger }).wrap()].spacing(3).width(Length::Fill)].spacing(14).align(Align::Center),
            row![
                input(p.admin.clone(), "Password", Msg::AdminPassword).secret().focused().on_submit(Msg::Unlock).width(Length::Fill),
                button("Cancel").on_press(Msg::CancelUnlock),
                button("Apply").primary().on_press(Msg::Unlock),
            ]
            .spacing(10)
            .align(Align::Center),
        ]
        .spacing(12)
        .padding([16, 18])
        .card()
        .into(),
    )
}

fn network(p: &Prefs) -> Stack<Msg> {
    let status = &p.net;
    if !status.available {
        return page(alloc::vec![note("This system has no network stack.")]);
    }
    let mode = match status.mode.as_str() {
        "ethernet" => 0,
        "wifi" => 1,
        _ => 2,
    };
    let mut children = alloc::vec![group("", alloc::vec![item("Network", "Which connection the computer uses", dropdown(["Wired (Ethernet)", "Wireless (Wi-Fi)", "Off"], Some(mode), Msg::NetMode).width(200u32))])];
    for iface in status.interfaces.iter() {
        let connected = iface.link && !iface.address.is_empty();
        let (state, tone) = if connected {
            (String::from("Connected"), Tone::Success)
        } else if iface.link {
            (String::from("Getting an address…"), Tone::Warning)
        } else {
            (String::from("Cable unplugged"), Tone::Dim)
        };
        let dash = |v: &str| if v.is_empty() { String::from("—") } else { String::from(v) };
        let title = if iface.kind == "wifi" { "Wi-Fi adapter" } else { "Wired connection" };
        children.push(group(
            title,
            alloc::vec![
                row![column![text(format!("{} · {}", iface.name, iface.driver)), text(iface.mac.clone()).small().tone(Tone::Dim)].spacing(3).width(Length::Fill), text(state).tone(tone)].spacing(18).align(Align::Center).into(),
                value("IP address", dash(&iface.address)),
                value("Router", dash(&iface.gateway)),
                value("DNS", dash(&iface.dns)),
            ],
        ));
    }
    if status.interfaces.is_empty() {
        children.push(note("No network adapter was found."));
    }
    if status.mode == "wifi" && status.has_kind("wifi") {
        let rows: Vec<Element<Msg>> = p
            .networks
            .iter()
            .map(|n| {
                let bars = match n.signal {
                    0..=25 => "ui/wifi-0",
                    26..=50 => "ui/wifi-1",
                    51..=75 => "ui/wifi-2",
                    _ => "ui/wifi-3",
                };
                let detail = if n.connected { String::from("Connected") } else if n.secured() { format!("Secured · {}%", n.signal) } else { format!("Open · {}%", n.signal) };
                let mut r = row![icon(bars).size(20u32, 20u32).tone(if n.connected { Tone::Success } else { Tone::Normal }), column![text(n.ssid.clone()), text(detail).small().tone(Tone::Dim)].spacing(3).width(Length::Fill)].spacing(14).align(Align::Center);
                if n.secured() {
                    r = r.push(icon("ui/lock").size(14u32, 14u32).tone(Tone::Faint));
                }
                r.into()
            })
            .collect();
        let count = rows.len();
        let mut wifi = column![
            row![text("Wi-Fi networks").strong().width(Length::Fill), button("Scan").icon("ui/refresh").flat().on_press(Msg::WifiScan)].align(Align::Center),
            column![list(rows).selected(p.wifi_selected).on_select(Msg::WifiPick).row_height(52).height(count.max(1) as u32 * 54 + 8)].padding([2, 2]).card(),
        ]
        .spacing(10);
        if count == 0 {
            wifi = wifi.push(note("No networks found yet. Press Scan to look again."));
        }
        if let Some(n) = p.wifi_selected.and_then(|i| p.networks.get(i)) {
            let action: Element<Msg> = if n.connected {
                row![text(format!("Connected to {}", n.ssid)).width(Length::Fill), button("Disconnect").on_press(Msg::WifiDisconnect)].spacing(10).align(Align::Center).into()
            } else if n.secured() {
                row![input(p.wifi_password.clone(), format!("Password for {}", n.ssid), Msg::WifiPassword).secret().focused().on_submit(Msg::WifiConnect).width(Length::Fill), button("Connect").primary().on_press(Msg::WifiConnect)].spacing(10).into()
            } else {
                row![text(format!("{} is an open network", n.ssid)).width(Length::Fill), button("Connect").primary().on_press(Msg::WifiConnect)].spacing(10).align(Align::Center).into()
            };
            wifi = wifi.push(group("", alloc::vec![action]));
        }
        children.push(wifi.into());
        if !p.saved_networks.is_empty() {
            let rows = p
                .saved_networks
                .iter()
                .map(|s| {
                    let ssid = s.ssid.clone();
                    item(&s.ssid, "Join automatically", row![switch(s.automatic, move |on| Msg::WifiAutojoin(ssid.clone(), on)), button("Forget").flat().on_press(Msg::WifiForget(s.ssid.clone()))].spacing(10).align(Align::Center))
                })
                .collect();
            children.push(group("Known networks", rows));
        }
    }
    if status.mode != "off" {
        let mut rows = alloc::vec![item("IP settings", "Automatic gets an address from the router", dropdown(["Automatic (DHCP)", "Manual"], Some(if p.dhcp { 0 } else { 1 }), Msg::Dhcp).width(200u32))];
        if !p.dhcp {
            rows.push(item("Address", "With the prefix, for example /24", input(p.address.clone(), "192.168.1.20/24", Msg::Address).width(220u32)));
            rows.push(item("Router", "", input(p.gateway.clone(), "192.168.1.1", Msg::Gateway).width(220u32)));
            rows.push(item("DNS server", "", input(p.dns.clone(), "1.1.1.1", Msg::Dns).width(220u32)));
        }
        rows.push(row![privilege_hint(), button("Apply").primary().on_press(Msg::ApplyNetwork)].spacing(12).align(Align::Center).into());
        children.push(group("Addresses", rows));
    }
    page(children)
}

fn display(p: &Prefs) -> Stack<Msg> {
    let info = &p.display;
    let mut children = Vec::new();
    if let Some(left) = info.revert_in {
        children.push(group(
            "",
            alloc::vec![row![
                column![text("Keep this display mode?").strong(), text(format!("The previous mode comes back in {} seconds.", left)).small().tone(Tone::Warning)].spacing(3).width(Length::Fill),
                button("Revert").on_press(Msg::RevertDisplay),
                button("Keep").primary().on_press(Msg::KeepDisplay),
            ]
            .spacing(10)
            .align(Align::Center)
            .into()],
        ));
    }
    if info.can_set {
        let sizes = p.sizes();
        let labels: Vec<String> = sizes.iter().map(|(w, h)| format!("{} × {}{}", w, h, if *w == info.native.width && *h == info.native.height { " (recommended)" } else { "" })).collect();
        let size = sizes.iter().position(|s| *s == (p.chosen.0, p.chosen.1));
        let rates = p.rates();
        let rate_labels: Vec<String> = rates.iter().map(|r| format!("{} Hz", r)).collect();
        let rate = rates.iter().position(|r| *r == p.chosen.2);
        let changed = p.chosen != (info.current.width, info.current.height, info.current.refresh);
        children.push(group(
            "",
            alloc::vec![
                item("Resolution", "", dropdown(labels, size, Msg::Resolution).width(230u32)),
                item("Refresh rate", "", dropdown(rate_labels, rate, Msg::Rate).width(230u32)),
                row![text("You get 15 seconds to keep or undo a new mode.").small().tone(Tone::Faint).width(Length::Fill), button("Apply").primary().on_press_maybe((changed && info.revert_in.is_none()).then_some(Msg::ApplyDisplay))].spacing(12).align(Align::Center).into(),
            ],
        ));
    } else {
        children.push(group("", alloc::vec![value("Resolution", format!("{} × {} at {} Hz", info.current.width, info.current.height, info.current.refresh)), note("This graphics adapter cannot change the resolution.")]));
    }
    if !info.connectors.is_empty() {
        let rows = info
            .connectors
            .iter()
            .map(|c| {
                let label = if c.monitor.is_empty() { c.name.clone() } else { format!("{} {}", c.vendor, c.monitor) };
                let detail = if !c.connected {
                    format!("{} · not connected", c.name)
                } else {
                    format!("{} · {} × {} at {} Hz{}", c.name, c.native.width, c.native.height, c.native.refresh, if c.primary { " · main display" } else { "" })
                };
                let action: Element<Msg> = if c.connected && !c.primary && info.can_set { button("Mirror").on_press(Msg::Mirror(c.id)).into() } else { hxtoolkit::space().into() };
                item(&label, &detail, action)
            })
            .collect();
        children.push(group("Screens", rows));
    }
    children.push(group("", alloc::vec![value("Graphics driver", format!("{} · {}", info.backend, info.output))]));
    page(children)
}

fn sound(p: &Prefs) -> Stack<Msg> {
    let info = &p.audio;
    let mut children = Vec::new();
    if !info.available {
        children.push(group("", alloc::vec![row![icon("ui/volume-off").size(24u32, 24u32).tone(Tone::Dim), note("No sound card was found. Supported: Intel High Definition Audio and AC'97.")].spacing(14).align(Align::Center).into()]));
        return page(children);
    }
    match p.volume.as_ref() {
        Some(v) => {
            let glyph = if v.muted || v.level == 0 { "ui/volume-mute" } else if v.level < 34 { "ui/volume-1" } else if v.level < 67 { "ui/volume-2" } else { "ui/volume-3" };
            children.push(group(
                "Output",
                alloc::vec![
                    row![icon(glyph).size(20u32, 20u32), slider(0.0..=100.0, v.level as f32, Msg::Volume).step(1.0).width(Length::Fill), text(if v.muted { String::from("Muted") } else { format!("{}%", v.level) }).tone(Tone::Dim).width(56u32)].spacing(14).align(Align::Center).into(),
                    item("Mute", "", switch(v.muted, Msg::Mute)),
                    item(&info.device, &format!("{} · {}", info.driver, if info.outputs.is_empty() { String::from("default output") } else { info.outputs.clone() }), button("Test").icon("ui/play").on_press(Msg::TestSound)),
                ],
            ));
        }
        None => children.push(note("The mixer cannot be opened. Allow Sound for Settings in Apps.")),
    }
    let state = if info.running { format!("Playing · {} stream{}", info.streams, if info.streams == 1 { "" } else { "s" }) } else { String::from("Idle") };
    children.push(group("", alloc::vec![value("Sample rate", format!("{} Hz", info.rate)), value("State", state)]));
    page(children)
}

fn tile(image: Element<Msg>, label: String, selected: bool) -> Element<Msg> {
    let caption = if selected { row![icon("ui/check").size(14u32, 14u32).tone(Tone::Accent), text(label).small().strong()].spacing(6).align(Align::Center) } else { row![text(label).small()] };
    column![column![image].width(system::THUMB_W as u32).height(system::THUMB_H as u32), caption].spacing(8).align(Align::Center).into()
}

fn appearance(p: &Prefs) -> Stack<Msg> {
    let styles: Vec<Element<Msg>> = ["Dark", "Light"].iter().enumerate().map(|(i, label)| tile(picture(p.styles[i].clone()).into(), String::from(*label), p.light == (i == 1))).collect();
    let walls: Vec<Element<Msg>> = p
        .wallpapers
        .iter()
        .map(|w| {
            let image: Element<Msg> = match &w.thumb {
                Some(t) => picture(t.clone()).into(),
                None => hxtoolkit::space().into(),
            };
            tile(image, w.title.clone(), w.path == p.wallpaper)
        })
        .collect();
    let current = p.wallpapers.iter().position(|w| w.path == p.wallpaper);
    let cell = (system::THUMB_W as u32 + 16, system::THUMB_H as u32 + 46);
    let credit = current.and_then(|i| p.wallpapers.get(i)).map(|w| format!("{} · {}", w.title, w.credit)).unwrap_or_default();
    page(alloc::vec![
        column![text("Style").strong(), list(styles).grid(cell.0, cell.1).selected(Some(p.light as usize)).on_select(Msg::Style)].spacing(10).into(),
        column![
            text("Wallpaper").strong(),
            list(walls).grid(cell.0, cell.1).selected(current).on_select(Msg::Wallpaper),
            note(if credit.is_empty() { String::from("Put your own PNG pictures in ~/Pictures to see them here.") } else { format!("{}. Put your own PNG pictures in ~/Pictures to see them here.", credit) }),
        ]
        .spacing(10)
        .into(),
    ])
}

fn desktop(p: &Prefs) -> Stack<Msg> {
    let size = DOCK_SIZES.iter().enumerate().min_by_key(|(_, (_, v))| (v - p.dock_size).abs()).map(|(i, _)| i).unwrap_or(1);
    let limit = DOCK_LIMITS.iter().position(|l| *l >= p.dock_limit).unwrap_or(2);
    let limits: Vec<String> = DOCK_LIMITS.iter().map(|l| format!("{} apps", l)).collect();
    let mut animation: Vec<Element<Msg>> = EFFECTS.iter().enumerate().map(|(i, e)| item(e.label, "", switch(p.effects[i], move |on| Msg::Effect(i, on)))).collect();
    let speed = SPEEDS.iter().position(|(_, v)| *v == p.speed).unwrap_or(1);
    animation.push(item("Speed", "", dropdown(SPEEDS.iter().map(|(l, _)| *l), Some(speed), Msg::Speed).width(160u32)));
    page(alloc::vec![
        group(
            "Dock",
            alloc::vec![
                item("Hide automatically", "The dock slides away when a window touches it", switch(p.autohide, Msg::Autohide)),
                item("Icon size", "", dropdown(DOCK_SIZES.iter().map(|(l, _)| *l), Some(size), Msg::DockSize).width(160u32)),
                item("Show at most", "The rest goes into the ••• menu", dropdown(limits, Some(limit), Msg::DockLimit).width(160u32)),
            ]
        ),
        group("Animations", animation),
    ])
}

fn summary(a: &crate::AppEntry) -> String {
    let labels: Vec<&str> = a.requested.iter().filter(|g| a.granted.contains(g)).filter_map(|g| permissions::find(g)).map(|p| p.label).collect();
    if a.requested.is_empty() {
        String::from("No special permissions")
    } else if labels.is_empty() {
        String::from("Everything turned off")
    } else {
        labels.join(", ")
    }
}

fn apps(p: &Prefs) -> Stack<Msg> {
    let rows: Vec<Element<Msg>> = p.apps.iter().map(|a| nav(&a.icon, &a.name, &summary(a))).collect();
    page(alloc::vec![
        note("Choose an application to see and change what it is allowed to do. Permissions can only take away rights your account has."),
        nav_group("", rows, Msg::PickApp),
    ])
}

fn app_detail(p: &Prefs, index: usize) -> Stack<Msg> {
    let Some(app) = p.apps.get(index) else {
        return page(Vec::new());
    };
    let entry = |group_name: &String| -> Option<Element<Msg>> {
        let perm = permissions::find(group_name)?;
        let allowed = app.granted.contains(group_name);
        let g = group_name.clone();
        Some(item(perm.label, perm.description, switch(allowed, move |on| Msg::Permit(g.clone(), on))))
    };
    let normal: Vec<Element<Msg>> = app.requested.iter().filter(|g| !app.required.contains(g)).filter(|g| permissions::find(g).map(|p| !p.superpermission).unwrap_or(false)).filter_map(entry).collect();
    let superior: Vec<Element<Msg>> = app.requested.iter().filter(|g| !app.required.contains(g)).filter(|g| permissions::find(g).map(|p| p.superpermission).unwrap_or(false)).filter_map(entry).collect();
    let toggles = !normal.is_empty() || !superior.is_empty();
    let mut children = alloc::vec![row![icon(app.icon.clone()).size(48u32, 48u32), column![text(app.name.clone()).size(TextSize::Strong), text(app.id.clone()).small().tone(Tone::Dim).selectable()].spacing(4)].spacing(16).align(Align::Center).into()];
    if app.requested.is_empty() {
        children.push(group("", alloc::vec![note("This application does not ask for any permissions, so it cannot use the network, sound or devices directly.")]));
    }
    if !normal.is_empty() {
        children.push(group("Permissions", normal));
    }
    if !superior.is_empty() {
        children.push(group("Superpermissions", superior));
    }
    if !app.required.is_empty() {
        let labels: Vec<&str> = app.required.iter().filter_map(|g| permissions::find(g)).map(|p| p.label).collect();
        children.push(note(format!("{} always keeps {} because it cannot run without them.", app.name, labels.join(" and "))));
    }
    if !app.declared {
        children.push(note("This program does not list its permissions, so it gets the ones desktop programs usually need."));
    }
    if toggles {
        children.push(row![privilege_hint(), button("Reset").on_press(Msg::ResetPermissions)].spacing(12).align(Align::Center).into());
        children.push(note("Changes take effect the next time the application starts."));
    }
    page(children)
}

fn users_page(p: &Prefs) -> Stack<Msg> {
    let user = system::user();
    let admin = sys::geteuid() == 0 || system::is_admin(&user);
    let mut groups = alloc::vec![users::find("", &user).and_then(|u| users::group_ids().into_iter().find(|(_, id)| *id == u.gid).map(|(n, _)| n)).unwrap_or_else(|| user.clone())];
    groups.extend(users::member_of("", &user));
    let mut children = alloc::vec![
        group(
            "",
            alloc::vec![
                row![icon("ui/user").size(40u32, 40u32), column![text(user.clone()).size(TextSize::Strong), text(if admin { "Administrator" } else { "Standard user" }).small().tone(Tone::Dim)].spacing(4).width(Length::Fill)].spacing(16).align(Align::Center).into(),
                value("Home folder", system::home()),
                value("Groups", groups.join(", ")),
            ]
        ),
        group(
            "Password",
            alloc::vec![
                item("Current password", "", input(p.current.clone(), "", Msg::Current).secret().width(240u32)),
                item("New password", "", input(p.new.clone(), "", Msg::New).secret().width(240u32)),
                item("Repeat the new password", "", input(p.repeat.clone(), "", Msg::Repeat).secret().on_submit(Msg::ChangePassword).width(240u32)),
                row![hxtoolkit::space(), button("Change password").primary().on_press(Msg::ChangePassword)].into(),
            ]
        ),
    ];
    let others = system::other_users();
    if !others.is_empty() {
        let rows = others.iter().map(|u| item(&u.name, if system::is_admin(&u.name) { "Administrator" } else { "Standard user" }, hxtoolkit::space())).collect();
        children.push(group("Other users", rows));
    }
    children.push(note("Administrators may power off the computer and change system settings without typing a password. Accounts are kept in /etc/passwd and /etc/group."));
    page(children)
}

fn system_page() -> Stack<Msg> {
    let info = sys::sysinfo();
    page(alloc::vec![
        row![icon("logo").size(56u32, 56u32), column![text("HamixOS 0.6.1").size(TextSize::Title), text(hostname()).tone(Tone::Dim)].spacing(4)].spacing(18).align(Align::Center).into(),
        group("", alloc::vec![value("Memory", system::human_size(info.mem_total)), value("Running for", format!("{} minutes", info.uptime_ms / 60000))]),
        nav_group(
            "",
            alloc::vec![
                nav("ui/info", "About this computer", "Processor, graphics, memory and disks"),
                nav("ui/session", "Startup & Linux programs", "What happens after login, Linux compatibility"),
                nav("ui/system", "Boot status", "Services started at boot"),
            ],
            |i| Msg::Open([Sub::About, Sub::Startup, Sub::Boot][i.min(2)]),
        ),
    ])
}

fn about() -> Stack<Msg> {
    let info = sys::sysinfo();
    let cpu = fs::read_to_string("/proc/cpuinfo").and_then(|t| t.lines().find(|l| l.starts_with("model name")).map(|l| String::from(l.split(':').nth(1).unwrap_or("").trim()))).unwrap_or_default();
    let cores = sys::cpu_stats().len();
    let gpu = fs::read_to_string("/proc/gpu").and_then(|t| t.lines().next().map(|l| String::from(l.split('\t').nth(1).unwrap_or("")))).unwrap_or_default();
    let root = fs::read_to_string("/proc/mounts").and_then(|t| t.lines().next().map(|l| String::from(l.split('\t').next().unwrap_or("")))).unwrap_or_default();
    let disks: Vec<String> = sys::disk_listing()
        .lines()
        .filter(|l| l.starts_with("disk"))
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            format!("{} · {}", f.get(1).unwrap_or(&""), system::human_size(f.get(2).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0) * 512))
        })
        .collect();
    let root = if root == "live" { String::from("Live image in memory") } else if root.starts_with("live:") { String::from("Live system from the boot disc") } else { root };
    page(alloc::vec![
        group(
            "Hardware",
            alloc::vec![
                value("Processor", format!("{} × {}", cores, cpu)),
                value("Graphics", gpu),
                value("Memory", format!("{} ({} in use)", system::human_size(info.mem_total), system::human_size(info.mem_used()))),
                value("Disks", if disks.is_empty() { String::from("None") } else { disks.join(", ") }),
            ]
        ),
        group("Software", alloc::vec![value("Operating system", "HamixOS 0.6.1"), value("Desktop", "Nook"), value("System disk", root), value("Device name", hostname())]),
    ])
}

fn startup(p: &Prefs) -> Stack<Msg> {
    let state = match system::linux_state().as_str() {
        "active" => format!("Running · {} Linux process(es)", system::linux_processes()),
        "on" => String::from("Ready, starts with the first Linux program"),
        _ => String::from("Turned off"),
    };
    let linux: Element<Msg> = if p.linux_available { item("Run Linux programs", &state, switch(p.linux, Msg::Linux)) } else { value("Run Linux programs", "Not installed") };
    let changed = p.autostart != p.autostart_saved || p.linux != p.linux_saved;
    page(alloc::vec![
        group("After login", alloc::vec![item("Start the desktop automatically", "Otherwise the text console opens and you can type startx", switch(p.autostart, Msg::Autostart))]),
        group("Linux compatibility", alloc::vec![linux]),
        row![privilege_hint(), button("Apply").primary().on_press_maybe(changed.then_some(Msg::ApplyStartup))].spacing(12).align(Align::Center).into(),
    ])
}

fn boot() -> Stack<Msg> {
    let text_units = fs::read_to_string("/proc/hxinit").unwrap_or_default();
    let units: Vec<Vec<String>> = text_units.lines().map(|l| l.split('\t').map(String::from).collect()).filter(|f: &Vec<String>| f.len() >= 5).collect();
    let failed = units.iter().filter(|u| u[1] == "fail").count();
    let rows: Vec<Element<Msg>> = units
        .iter()
        .map(|u| {
            let (label, tone) = match u[1].as_str() {
                "ok" => ("Started", Tone::Success),
                "warn" => ("Warning", Tone::Warning),
                "fail" => ("Failed", Tone::Danger),
                _ => ("Skipped", Tone::Faint),
            };
            row![column![text(u[3].clone()), text(u[4].clone()).small().tone(Tone::Dim)].spacing(3).width(Length::Fill), text(format!("{} ms", u[2])).small().tone(Tone::Faint), text(label).small().strong().tone(tone).width(72u32)].spacing(14).align(Align::Center).into()
        })
        .collect();
    let headline = if failed == 0 { format!("All {} services started", units.len()) } else { format!("{} of {} services failed", failed, units.len()) };
    page(alloc::vec![group(&headline, if rows.is_empty() { alloc::vec![note("No boot information is available.")] } else { rows })])
}
