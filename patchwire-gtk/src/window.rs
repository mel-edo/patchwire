use std::sync::{Arc, Mutex};
use gtk4::prelude::*;
use gtk4::{Align, ScrolledWindow, PolicyType, Box as GBox, Orientation, StringList, glib};
use libadwaita as adw;
use adw::prelude::*;
use tokio::runtime::Handle;

use crate::dbus_client::{PatchwireDaemonProxy, SinkInfo};

pub struct PatchwireWindow {
    pub window: adw::ApplicationWindow,
}

impl PatchwireWindow {
    pub fn new(
        app: &adw::Application,
        proxy: PatchwireDaemonProxy<'static>,
        rt: Handle,
    ) -> Self {
        let proxy = Arc::new(proxy);
        // root window
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Patchwire")
            .default_width(420)
            .default_height(560)
            .build();

        // toolbar view
        let toolbar_view = adw::ToolbarView::new();
        window.set_content(Some(&toolbar_view));

        // header bar
        let header = adw::HeaderBar::new();
        header.set_decoration_layout(Some(":close"));
        header.add_css_class("flat");
        toolbar_view.add_top_bar(&header);

        // scrolled + clamp
        let scroll = ScrolledWindow::builder()
            .hscrollbar_policy(PolicyType::Never)
            .vscrollbar_policy(PolicyType::Automatic)
            .build();
        toolbar_view.set_content(Some(&scroll));

        let clamp = adw::Clamp::builder()
            .maximum_size(600)
            .build();
        scroll.set_child(Some(&clamp));

        let outer_box = GBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(12)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();
        clamp.set_child(Some(&outer_box));

        // sinks group
        let sink_group = adw::PreferencesGroup::new();
        sink_group.set_title("Audio Sinks");
        sink_group.set_description(Some("Toggle routing to secondary outputs"));
        outer_box.append(&sink_group);

        // default sink row
        let default_row = adw::ActionRow::new();
        default_row.set_title("Default sink");
        default_row.set_subtitle("Loading...");
        let icon = gtk4::Image::from_icon_name("audio-volume-high-symbolic");
        default_row.add_prefix(&icon);
        let default_btn = gtk4::Button::with_label("Change");
        default_btn.set_valign(Align::Center);
        default_btn.add_css_class("flat");
        default_row.add_suffix(&default_btn);
        sink_group.add(&default_row);

        // profiles group
        let profile_group = adw::PreferencesGroup::new();
        profile_group.set_title("Profiles");
        outer_box.append(&profile_group);

        let profile_combo = adw::ComboRow::new();
        profile_combo.set_title("Active profile");
        profile_combo.set_model(Some(&gtk4::StringList::new(&[])));
        profile_group.add(&profile_combo);

        let save_row = adw::ActionRow::new();
        save_row.set_title("Save current state as profile");
        let save_btn = gtk4::Button::with_label("Save...");
        save_btn.set_valign(Align::Center);
        save_btn.add_css_class("suggested-action");
        save_row.add_suffix(&save_btn);
        profile_group.add(&save_row);

        let del_row = adw::ActionRow::new();
        del_row.set_title("Delete active profile");
        let del_btn = gtk4::Button::with_label("Delete");
        del_btn.set_valign(Align::Center);
        del_btn.add_css_class("destructive-action");
        del_row.add_suffix(&del_btn);
        profile_group.add(&del_row);

        // shared state - store sink rows so we can update them from singal callbacks
        let sink_rows: Arc<Mutex<Vec<(String, adw::SwitchRow)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let handler_ids: Arc<Mutex<Vec<(String, glib::SignalHandlerId)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let profile_updating = Arc::new(Mutex::new(false));

        // initial data fetch
        {
            let proxy = Arc::clone(&proxy);
            let sink_group = sink_group.clone();
            let default_row = default_row.clone();
            let sink_rows = Arc::clone(&sink_rows);
            let profile_combo = profile_combo.clone();
            let rt2 = rt.clone();

            let (sinks, profiles) = {
                let proxy2 = Arc::clone(&proxy);
                rt.block_on(async move {
                    let sinks = proxy2.list_sinks().await.unwrap_or_default();
                    let profiles = proxy2.get_profiles().await.unwrap_or_default();
                    (sinks, profiles)
                })
            };

            populate_sinks(
                &sink_group,
                &default_row,
                &sink_rows,
                &handler_ids,
                sinks,
                Arc::clone(&proxy),
                rt2,
            );
            populate_profiles(&profile_combo, profiles, &profile_updating);
        }

        // profile combo selection
        {
            let proxy = Arc::clone(&proxy);
            let rt2 = rt.clone();
            let profile_updating = Arc::clone(&profile_updating);
            profile_combo.connect_selected_notify(move |combo| {
                if *profile_updating.lock().unwrap() {
                    return;
                }
                let idx = combo.selected() as usize;
                let model = combo.model().unwrap();
                let list = model.downcast::<StringList>().unwrap();
                if let Some(name) = list.string(idx as u32) {
                    let name = name.to_string();
                    let proxy2 = Arc::clone(&proxy);
                    rt2.spawn(async move {
                        if let Err(e) = proxy2.set_active_profile(&name).await {
                            eprintln!("set_active_profile failed: {e}");
                        }
                    });
                }
            });
        }

        // signal subscriptions
        {
            let proxy_for_signal = Arc::clone(&proxy);
            let proxy_for_default_signal = Arc::clone(&proxy);
            let sink_rows = Arc::clone(&sink_rows);
            let sink_rows_for_default = Arc::clone(&sink_rows);
            let handler_ids_for_default = Arc::clone(&handler_ids);
            let sink_group_for_default = sink_group.clone();
            let default_row_for_default = default_row.clone();

            // channel carries plain Vec<SinkInfo> from tokio to glib main context
            let (tx, rx) = std::sync::mpsc::channel::<Vec<crate::dbus_client::SinkInfo>>();
            let tx_for_default = tx.clone();

            // listen for SinksChanged
            rt.spawn(async move {
                use futures_util::StreamExt;
                if let Ok(mut stream) = proxy_for_signal.receive_sinks_changed().await {
                    while stream.next().await.is_some() {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        let sinks = proxy_for_signal.list_sinks().await.unwrap_or_default();
                        let _ = tx.send(sinks);
                    }
                }
            });

            // listen for DefaultChanged
            let proxy_for_default2 = Arc::clone(&proxy_for_default_signal);
            rt.spawn(async move {
                use futures_util::StreamExt;
                if let Ok(mut stream) = proxy_for_default2.receive_default_changed().await {
                    while stream.next().await.is_some() {
                        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                        let sinks = proxy_for_default2.list_sinks().await.unwrap_or_default();
                        let _ = tx_for_default.send(sinks);
                    }
                }
            });

            let proxy_for_poll = Arc::clone(&proxy);
            let rt_for_poll = rt.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                while let Ok(sinks) = rx.try_recv() {
                    populate_sinks(
                        &sink_group_for_default,
                        &default_row_for_default,
                        &sink_rows_for_default,
                        &handler_ids_for_default,
                        sinks,
                        Arc::clone(&proxy_for_poll),
                        rt_for_poll.clone(),
                    );
                }
                glib::ControlFlow::Continue
            });

            let handler_ids_for_state = Arc::clone(&handler_ids);
            let _ = handler_ids_for_state;
        }

        // save button
        {
            let proxy = Arc::clone(&proxy);
            let rt2 = rt.clone();
            let profile_combo = profile_combo.clone();
            let profile_updating = Arc::clone(&profile_updating);

            let popover = gtk4::Popover::new();
            popover.set_parent(&save_btn);

            let pop_box = GBox::builder()
                .orientation(Orientation::Vertical)
                .spacing(8)
                .margin_top(8)
                .margin_bottom(8)
                .margin_start(8)
                .margin_end(8)
                .build();
            popover.set_child(Some(&pop_box));

            let entry = gtk4::Entry::new();
            entry.set_placeholder_text(Some("Profile name..."));
            entry.set_width_chars(20);
            pop_box.append(&entry);

            let confirm_btn = gtk4::Button::with_label("Save");
            confirm_btn.add_css_class("suggested-action");
            pop_box.append(&confirm_btn);

            // open popover on click, clear previous text
            {
                let popover = popover.clone();
                let entry = entry.clone();
                save_btn.connect_clicked(move |_| {
                    entry.set_text("");
                    popover.popup();
                });
            }

            {
                let confirm_btn = confirm_btn.clone();
                entry.connect_activate(move |_| {
                    confirm_btn.emit_clicked();
                });
            }

            // confirm
            {
                let popover = popover.clone();
                let entry = entry.clone();
                confirm_btn.connect_clicked(move |_| {
                    let name = entry.text().trim().to_string();
                    if name.is_empty() {
                        return;
                    }
                    popover.popdown();
                    let proxy2 = Arc::clone(&proxy);
                    let profile_combo2 = profile_combo.clone();
                    let profile_updating2 = Arc::clone(&profile_updating);
                    let profiles = rt2.block_on(async move {
                        if let Err(e) = proxy2.save_profile(&name).await {
                            eprintln!("save_profile failed: {e}");
                            return vec![];
                        }
                        proxy2.get_profiles().await.unwrap_or_default()
                    });
                    populate_profiles(&profile_combo2, profiles, &profile_updating2);
                });
            }
        };

        // delete button
        {
            let proxy = Arc::clone(&proxy);
            let rt2 = rt.clone();
            let profile_combo = profile_combo.clone();
            let profile_updating = Arc::clone(&profile_updating);

            let popover = gtk4::Popover::new();
            popover.set_parent(&del_btn);

            let pop_box = GBox::builder()
                .orientation(Orientation::Vertical)
                .spacing(8)
                .margin_top(8)
                .margin_bottom(8)
                .margin_start(8)
                .margin_end(8)
                .build();
            popover.set_child(Some(&pop_box));

            let label = gtk4::Label::new(Some("Delete this profile?"));
            pop_box.append(&label);

            let confirm_btn = gtk4::Button::with_label("Delete");
            confirm_btn.add_css_class("destructive-action");
            pop_box.append(&confirm_btn);

            {
                let popover = popover.clone();
                del_btn.connect_clicked(move |_| {
                    popover.popup();
                });
            }

            {
                let popover = popover.clone();
                let profile_combo = profile_combo.clone();
                confirm_btn.connect_clicked(move |_| {
                    let idx = profile_combo.selected() as u32;
                    let model = profile_combo.model().unwrap();
                    let list = model.downcast::<StringList>().unwrap();
                    let name = match list.string(idx) {
                        Some(n) => n.to_string(),
                        None => return,
                    };
                    popover.popdown();
                    let proxy2 = Arc::clone(&proxy);
                    let profile_combo2 = profile_combo.clone();
                    let profile_updating2 = Arc::clone(&profile_updating);
                    let profiles = rt2.block_on(async move {
                        if let Err(e) = proxy2.delete_profile(&name).await {
                            eprintln!("delete_profile failed: {e}");
                            return vec![];
                        }
                        proxy2.get_profiles().await.unwrap_or_default()
                    });
                    populate_profiles(&profile_combo2, profiles, &profile_updating2);
                });
            }
        }

        // default sink popover
        {
            let proxy = Arc::clone(&proxy);
            let rt2 = rt.clone();
            let default_row = default_row.clone();

            let popover = gtk4::Popover::new();
            popover.set_parent(&default_btn);

            let pop_box = GBox::builder()
                .orientation(Orientation::Vertical)
                .spacing(4)
                .margin_top(8)
                .margin_bottom(8)
                .margin_start(8)
                .margin_end(8)
                .build();
            popover.set_child(Some(&pop_box));

            // fetch all sinks and build a button for each non default one
            let all_sinks: Arc<Mutex<Vec<SinkInfo>>> = Arc::new(Mutex::new(Vec::new()));

            {
                let all_sinks = Arc::clone(&all_sinks);
                let sinks = rt2.block_on(async {
                    proxy.list_sinks().await.unwrap_or_default()
                });
                *all_sinks.lock().unwrap() = sinks;
            }

            {
                let popover = popover.clone();
                let all_sinks = Arc::clone(&all_sinks);
                let proxy = Arc::clone(&proxy);
                let rt2 = rt.clone();
                let default_row = default_row.clone();
                let pop_box = pop_box.clone();

                default_btn.connect_clicked(move |_| {
                    // clear old button
                    while let Some(child) = pop_box.first_child() {
                        pop_box.remove(&child);
                    }

                    let sinks = all_sinks.lock().unwrap().clone();
                    for sink in sinks {
                        let btn = gtk4::Button::with_label(&sink.description);
                        btn.add_css_class("flat");
                        if sink.is_default {
                            btn.add_css_class("accent");
                        }

                        let proxy2 = Arc::clone(&proxy);
                        let rt3 = rt2.clone();
                        let popover2 = popover.clone();
                        let default_row2 = default_row.clone();
                        let desc = sink.description.clone();
                        let name = sink.name.clone();

                        btn.connect_clicked(move |_| {
                            popover2.popdown();
                            let proxy3 = Arc::clone(&proxy2);
                            let name2 = name.clone();
                            let desc2 = desc.clone();
                            let default_row3 = default_row2.clone();
                            let success = rt3.block_on(async move {
                                match proxy3.set_default_sink(&name2).await {
                                    Ok(_) => true,
                                    Err(e) => {
                                        eprintln!("set_default_sink failed: {e}");
                                        false
                                    }
                                }
                            });
                            if success {
                                default_row3.set_subtitle(&desc2);
                            }
                        });
                        pop_box.append(&btn);
                    }
                    popover.popup();
                });
            }

            let (tx2, rx2) = std::sync::mpsc::channel::<Vec<SinkInfo>>();
            let proxy_for_default = Arc::clone(&proxy);
            rt.spawn(async move {
                use futures_util::StreamExt;
                if let Ok(mut steam) = proxy_for_default.receive_sinks_changed().await {
                    while steam.next().await.is_some() {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        let sinks = proxy_for_default.list_sinks().await.unwrap_or_default();
                        let _ = tx2.send(sinks);
                    }
                }
            });

            glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                while let Ok(sinks) = rx2.try_recv() {
                    *all_sinks.lock().unwrap() = sinks;
                }
                glib::ControlFlow::Continue
            });
        }

        Self { window }
    }

    pub fn present(&self) {
        self.window.present();
    }
}

// helpers
fn populate_sinks(
    sink_group: &adw::PreferencesGroup,
    default_row: &adw::ActionRow,
    sink_rows: &Arc<Mutex<Vec<(String, adw::SwitchRow)>>>,
    handler_ids: &Arc<Mutex<Vec<(String, glib::SignalHandlerId)>>>,
    sinks: Vec<SinkInfo>,
    proxy: Arc<PatchwireDaemonProxy<'static>>,
    rt: Handle,
) {
    // clear old rows
    let mut rows = sink_rows.lock().unwrap();
    let mut ids = handler_ids.lock().unwrap();

    for (_, row) in rows.iter() {
        sink_group. remove(row);
    }
    rows.clear();
    ids.clear();

    for sink in sinks {
        if sink.is_default {
            default_row.set_subtitle(&sink.description);
            continue;
        }

        let row = adw::SwitchRow::new();
        row.set_title(&sink.description);
        row.set_subtitle(&sink.name);

        // wire toggle
        let name = sink.name.clone();
        let proxy2 = Arc::clone(&proxy);
        let rt2 = rt.clone();
        let handler_id = row.connect_active_notify(move |r| {
            let enabled = r.is_active();
            let name = name.clone();
            let proxy3 = Arc::clone(&proxy2);
            rt2.spawn(async move {
                if let Err(e) = proxy3.set_sink_enabled(&name, enabled).await {
                    eprintln!("set_sink_enabled failed: {e}");
                }
            });
        });

        row.block_signal(&handler_id);
        row.set_active(sink.is_enabled);
        row.unblock_signal(&handler_id);

        ids.push((sink.name.clone(), handler_id));
        sink_group.add(&row);
        rows.push((sink.name.clone(), row));
    }
}

fn populate_profiles(combo: &adw::ComboRow, profiles: Vec<String>, updating: &Arc<Mutex<bool>>) {
    *updating.lock().unwrap() = true;
    let list: Vec<&str> = profiles.iter().map(|s| s.as_str()).collect();
    combo.set_model(Some(&StringList::new(&list)));
    *updating.lock().unwrap() = false;
}
