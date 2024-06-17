// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: LGPL-3.0-or-later

use crate::dialogs::{alert, ConfirmDialog, DangerDialog, YES};
use crate::git::{commit, git_log};
use async_channel::Sender;
use core::time::Duration;
use git2::Oid;
use glib::{clone, Object};
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{
    gdk, gio, glib, pango, Box, Button, EventControllerKey, GestureClick,
    Image, Label, ListItem, ListView, Orientation, PositionType,
    ScrolledWindow, SearchBar, SearchEntry, SignalListItemFactory,
    SingleSelection, Widget, Window as Gtk4Window,
};
use libadwaita::prelude::*;
use libadwaita::{HeaderBar, ToolbarView, Window};
use log::trace;
use std::cell::RefCell;

use std::path::PathBuf;
use std::rc::Rc;

glib::wrapper! {
    pub struct CommitItem(ObjectSubclass<commit_item::CommitItem>);
}

mod commit_item {
    use crate::git::commit;
    use glib::Properties;
    use gtk4::glib;
    use gtk4::prelude::*;
    use gtk4::subclass::prelude::*;
    use std::cell::RefCell;

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::CommitItem)]
    pub struct CommitItem {
        pub commit: RefCell<commit::CommitLog>,

        #[property(get = Self::get_author)]
        pub author: String,

        #[property(get = Self::get_oid)]
        pub oid: String,

        #[property(get = Self::get_from)]
        pub from: String,

        #[property(get = Self::get_from_tooltip)]
        pub from_tooltip: String,

        #[property(get = Self::get_message)]
        pub message: String,

        #[property(get = Self::get_dt)]
        pub dt: String,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CommitItem {
        const NAME: &'static str = "StageCommitItem";
        type Type = super::CommitItem;
    }
    #[glib::derived_properties]
    impl ObjectImpl for CommitItem {}

    impl CommitItem {
        pub fn get_oid(&self) -> String {
            format!(
                "<span color=\"#1C71D8\"> {}</span>",
                self.commit.borrow().oid
            )
        }
        pub fn get_from(&self) -> String {
            match self.commit.borrow().from {
                commit::CommitRelation::None => "".to_string(),
                commit::CommitRelation::Left(_) => {
                    "mail-forward-symbolic".to_string()
                }
                commit::CommitRelation::Right(_) => {
                    "mail-reply-sender-symbolic".to_string()
                }
            }
        }
        pub fn get_from_tooltip(&self) -> String {
            match &self.commit.borrow().from {
                commit::CommitRelation::None => "".to_string(),
                commit::CommitRelation::Left(m) => m.to_string(),
                commit::CommitRelation::Right(m) => m.to_string(),
            }
        }

        pub fn get_author(&self) -> String {
            self.commit.borrow().author.to_string()
        }

        pub fn get_message(&self) -> String {
            self.commit.borrow().message.to_string()
        }
        pub fn get_dt(&self) -> String {
            self.commit.borrow().commit_dt.to_string()
        }
    }
}

impl CommitItem {
    pub fn new(commit: commit::CommitLog) -> Self {
        let ob = Object::builder::<CommitItem>().build();
        ob.imp().commit.replace(commit);
        ob
    }
}

glib::wrapper! {
    pub struct CommitList(ObjectSubclass<commit_list::CommitList>)
        @implements gio::ListModel;
}

mod commit_list {

    use glib::Properties;
    use gtk4::gio;
    use gtk4::glib;
    use gtk4::prelude::*;
    use gtk4::subclass::prelude::*;
    use std::cell::RefCell;

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::CommitList)]
    pub struct CommitList {
        pub list: RefCell<Vec<super::CommitItem>>,
        pub original_list: RefCell<Vec<super::commit::CommitLog>>,
        pub search_term: RefCell<(String, usize)>,

        // does not used for now
        #[property(get, set)]
        pub selected_pos: RefCell<u32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CommitList {
        const NAME: &'static str = "StageCommitList";
        type Type = super::CommitList;
        type ParentType = glib::Object;
        type Interfaces = (gio::ListModel,);
    }

    #[glib::derived_properties]
    impl ObjectImpl for CommitList {}

    impl ListModelImpl for CommitList {
        fn item_type(&self) -> glib::Type {
            super::CommitItem::static_type()
        }

        fn n_items(&self) -> u32 {
            self.list.borrow().len() as u32
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            let list = self.list.borrow();
            if list.is_empty() {
                return None;
            }
            if position as usize >= list.len() {
                return None;
            }
            // why clone???
            Some(list[position as usize].clone().into())
        }
    }
}

impl Default for CommitList {
    fn default() -> Self {
        Self::new()
    }
}

impl CommitList {
    pub fn new() -> Self {
        Object::builder().build()
    }
    pub fn get_commits_inside(
        &self,
        repo_path: PathBuf,
        mut start_oid: Option<Oid>,
        widget: &impl IsA<Widget>,
    ) {
        glib::spawn_future_local({
            let commit_list = self.clone();
            let repo_path = repo_path.clone();
            let widget = widget.clone();
            // let (ref term, term_count) = *self.imp().search_term.borrow();
            let (term, term_count) = self.imp().search_term.take();
            let search_term = {
                if term.is_empty() {
                    None
                } else {
                    // search pull commits 1 by 1. inc counter
                    // to stop that iteration when page size is reached
                    // self.imp().search_term.borrow_mut().1 = term_count + 1;
                    // Some(String::from(term))
                    self.imp()
                        .search_term
                        .replace((term.clone(), term_count + 1));
                    Some(term)
                }
            };
            async move {
                let list_le = commit_list.imp().list.borrow().len() as u32;
                let mut append_to_existing = false;
                if list_le > 0 {
                    let item = commit_list.item(list_le - 1).unwrap();
                    let commit_item =
                        item.downcast_ref::<CommitItem>().unwrap();
                    let oid = commit_item.imp().commit.borrow().oid;
                    start_oid.replace(oid);
                    append_to_existing = true;
                }
                let commits = gio::spawn_blocking({
                    let search_term = search_term.clone();
                    let repo_path = repo_path.clone();
                    move || git_log::revwalk(repo_path, start_oid, search_term)
                })
                .await
                .unwrap_or_else(|e| {
                    alert(format!("{:?}", e)).present(&widget);
                    Ok(Vec::new())
                })
                .unwrap_or_else(|e| {
                    alert(e).present(&widget);
                    Vec::new()
                });
                // trace!("commits in response {:?}", commits.len());
                if commits.is_empty() {
                    return;
                }
                let mut added = 0;
                let mut last_added_oid: Option<Oid> = None;
                for item in commits
                    .into_iter()
                    .map(|commit| {
                        if search_term.is_none() {
                            commit_list
                                .imp()
                                .original_list
                                .borrow_mut()
                                .push(commit.clone());
                        }
                        commit
                    })
                    .map(CommitItem::new)
                {
                    if append_to_existing {
                        if let Some(oid) = start_oid {
                            if item.imp().commit.borrow().oid == oid {
                                // trace!("skip previously found commit {:?}", oid);
                                continue;
                            }
                        }
                    }
                    last_added_oid.replace(item.imp().commit.borrow().oid);
                    commit_list.imp().list.borrow_mut().push(item);
                    added += 1;
                }
                if added > 0 {
                    commit_list.items_changed(
                        if list_le > 0 { list_le } else { 0 },
                        0,
                        added,
                    );
                    // search will return commits 1 by 1
                    // when got 1 commit and got for another
                    // it need to stop somehow
                    if search_term.is_some()
                        && last_added_oid.is_some()
                        && term_count < git_log::COMMIT_PAGE_SIZE
                    {
                        trace!(
                            "go next loop with start >>>>>>>>   oid {:?}",
                            last_added_oid
                        );
                        commit_list.get_commits_inside(
                            repo_path,
                            last_added_oid,
                            &widget,
                        );
                    }
                }
            }
        });
    }

    pub fn reset_search(&self) {
        self.imp().search_term.take();
        let orig_le = self.imp().original_list.borrow().len();
        if orig_le == 0 {
            // this is hack for the first triggered event.
            // for some reason it is triggered without
            return;
        }
        let searched = self.imp().list.take();
        self.items_changed(0, searched.len() as u32, 0);
        self.imp().list.replace(
            self.imp()
                .original_list
                .borrow()
                .iter()
                .cloned()
                .map(CommitItem::new)
                .collect(),
        );
        self.items_changed(0, 0, self.imp().list.borrow().len() as u32);
    }

    pub fn search(
        &self,
        term: String,
        repo_path: PathBuf,
        widget: &impl IsA<Widget>,
    ) {
        self.imp().search_term.replace((term, 0));
        let current_length = self.imp().list.borrow().len();
        self.imp().list.borrow_mut().clear();
        self.items_changed(0, current_length as u32, 0);
        self.get_commits_inside(repo_path, None, widget);
    }

    pub fn get_selected_oid(&self) -> Oid {
        let pos = self.selected_pos();
        let item = self.item(pos).unwrap();
        let commit_item = item.downcast_ref::<CommitItem>().unwrap();
        let oid = commit_item.imp().commit.borrow().oid;
        oid
    }

    pub fn cherry_pick(
        &self,
        repo_path: PathBuf,
        window: &impl IsA<Widget>,
        sender: Sender<crate::Event>,
    ) {
        glib::spawn_future_local({
            let sender = sender.clone();
            let path = repo_path.clone();
            let window = window.clone();
            let oid = self.get_selected_oid();
            async move {
                let response = alert(ConfirmDialog(
                    "Cherry pick commit?".to_string(),
                    format!("{}", oid),
                ))
                .choose_future(&window)
                .await;
                if response != YES {
                    return;
                }
                gio::spawn_blocking({
                    let sender = sender.clone();
                    let path = path.clone();
                    move || commit::cherry_pick(path, oid, sender)
                })
                .await
                .unwrap_or_else(|e| {
                    alert(format!("{:?}", e)).present(&window);
                    Ok(())
                })
                .unwrap_or_else(|e| {
                    alert(e).present(&window);
                });
            }
        });
    }

    pub fn revert(
        &self,
        repo_path: PathBuf,
        window: &impl IsA<Widget>,
        sender: Sender<crate::Event>,
    ) {
        glib::spawn_future_local({
            let sender = sender.clone();
            let path = repo_path.clone();
            let window = window.clone();
            let oid = self.get_selected_oid();
            async move {
                let response = alert(ConfirmDialog(
                    "Revert commit?".to_string(),
                    format!("{}", oid),
                ))
                .choose_future(&window)
                .await;
                if response != YES {
                    return;
                }
                gio::spawn_blocking({
                    let sender = sender.clone();
                    let path = path.clone();
                    move || commit::revert(path, oid, sender)
                })
                .await
                .unwrap_or_else(|e| {
                    alert(format!("{:?}", e)).present(&window);
                    Ok(())
                })
                .unwrap_or_else(|e| {
                    alert(e).present(&window);
                });
            }
        });
    }

    pub fn reset_hard(
        &self,
        repo_path: PathBuf,
        window: &impl IsA<Widget>,
        sender: Sender<crate::Event>,
    ) {
        let oid = self.get_selected_oid();
        glib::spawn_future_local({
            let window = window.clone();
            let sender = sender.clone();
            let commit_list = self.clone();
            async move {
                let response = alert(DangerDialog(
                    String::from("Reset"),
                    format!("Hard reset to {}", oid),
                ))
                .choose_future(&window)
                .await;
                if response != YES {
                    return;
                }
                let result = gio::spawn_blocking({
                    let sender = sender.clone();
                    let path = repo_path.clone();
                    move || crate::reset_hard(path, Some(oid), sender)
                })
                .await
                .unwrap_or_else(|e| {
                    alert(format!("{:?}", e)).present(&window);
                    Ok(false)
                })
                .unwrap_or_else(|e| {
                    alert(e).present(&window);
                    false
                });
                if result {
                    loop {
                        // let original = *commit_list.imp().original_list.borrow_mut();
                        let first_oid =
                            commit_list.imp().original_list.borrow()[0].oid;
                        commit_list.imp().original_list.borrow_mut().remove(0);
                        if first_oid == oid {
                            break;
                        }
                    }
                    if commit_list.imp().search_term.borrow().0.is_empty() {
                        // remove from visual list only if it is not in search
                        let mut removed = 0;
                        loop {
                            // let original = *commit_list.imp().original_list.borrow_mut();
                            let first_oid = {
                                let first_item =
                                    &commit_list.imp().list.borrow()[0];
                                let first_oid =
                                    first_item.imp().commit.borrow().oid;
                                first_oid
                            };
                            if first_oid == oid {
                                break;
                            }
                            commit_list.imp().list.borrow_mut().remove(0);
                            removed += 1;
                        }
                        if removed > 0 {
                            commit_list.items_changed(0, removed, 0);
                        }
                    }
                }
            }
        });
    }
}

pub fn item_factory(sender: Sender<crate::Event>) -> SignalListItemFactory {
    let factory = SignalListItemFactory::new();
    let focus = Rc::new(RefCell::new(false));
    factory.connect_setup(move |_, list_item| {
        let oid_label = Label::builder()
            .label("")
            .use_markup(true)
            .width_chars(12)
            .max_width_chars(12)
            .xalign(0.0)
            .cursor(&gdk::Cursor::from_name("pointer", None).unwrap())
            .ellipsize(pango::EllipsizeMode::End)
            .build();
        let gesture_controller = GestureClick::new();
        gesture_controller.connect_released({
            let list_item = list_item.clone();
            let sender = sender.clone();
            move |_gesture, _some, _wx, _wy| {
                let list_item = list_item.downcast_ref::<ListItem>().unwrap();
                let commit_item = list_item.item().unwrap();
                let commit_item =
                    commit_item.downcast_ref::<CommitItem>().unwrap();
                let oid = commit_item.imp().commit.borrow().oid;
                sender
                    .send_blocking(crate::Event::ShowOid(oid, None))
                    .expect("cant send through sender");
            }
        });
        oid_label.add_controller(gesture_controller);

        let from = Image::new();

        let author_label = Label::builder()
            .label("")
            .width_chars(18)
            .max_width_chars(18)
            .xalign(0.0)
            .ellipsize(pango::EllipsizeMode::End)
            .build();

        let label_commit = Label::builder()
            .label("")
            .lines(1)
            .single_line_mode(true)
            .xalign(0.0)
            .width_chars(36)
            .max_width_chars(36)
            .ellipsize(pango::EllipsizeMode::End)
            .use_markup(true)
            .can_focus(true)
            .can_target(true)
            .build();

        let label_dt = Label::builder()
            .label("")
            .lines(1)
            .single_line_mode(true)
            .xalign(0.0)
            .width_chars(24)
            .max_width_chars(24)
            .ellipsize(pango::EllipsizeMode::End)
            .use_markup(true)
            .can_focus(true)
            .can_target(true)
            .build();

        let bx = Box::builder()
            .orientation(Orientation::Horizontal)
            // .css_classes(vec![String::from("branch_row")])
            .margin_top(2)
            .margin_bottom(2)
            .margin_start(2)
            .margin_end(2)
            .spacing(12)
            .can_focus(true)
            .focusable(true)
            .build();

        bx.append(&oid_label);
        bx.append(&from);
        bx.append(&author_label);
        bx.append(&label_commit);
        bx.append(&label_dt);

        let list_item = list_item
            .downcast_ref::<ListItem>()
            .expect("Needs to be ListItem");
        list_item.set_child(Some(&bx));
        list_item.set_selectable(true);
        list_item.set_activatable(true);
        list_item.set_focusable(true);

        let item = list_item.property_expression("item");
        item.chain_property::<CommitItem>("oid").bind(
            &oid_label,
            "label",
            Widget::NONE,
        );
        item.chain_property::<CommitItem>("from").bind(
            &from,
            "icon-name",
            Widget::NONE,
        );
        item.chain_property::<CommitItem>("from_tooltip").bind(
            &from,
            "tooltip-text",
            Widget::NONE,
        );

        item.chain_property::<CommitItem>("author").bind(
            &author_label,
            "label",
            Widget::NONE,
        );
        item.chain_property::<CommitItem>("message").bind(
            &label_commit,
            "label",
            Widget::NONE,
        );

        item.chain_property::<CommitItem>("dt").bind(
            &label_dt,
            "label",
            Widget::NONE,
        );
        let focus = focus.clone();
        list_item.connect_selected_notify(move |li: &ListItem| {
            glib::source::timeout_add_local(Duration::from_millis(300), {
                let focus = focus.clone();
                let li = li.clone();
                move || {
                    if !*focus.borrow() {
                        let first_child = li.child().unwrap();
                        let first_child =
                            first_child.downcast_ref::<Widget>().unwrap();
                        let row = first_child.parent().unwrap();
                        row.grab_focus();
                        *focus.borrow_mut() = true;
                    }
                    glib::ControlFlow::Break
                }
            });
        });
    });

    factory
}

pub fn listview_factory(sender: Sender<crate::Event>) -> ListView {
    let commit_list = CommitList::new();
    let selection_model = SingleSelection::new(Some(commit_list));

    // model IS commit_list actually
    let model = selection_model.model().unwrap();
    let bind =
        selection_model.bind_property("selected", &model, "selected_pos");
    let _ = bind.bidirectional().build();

    let factory = item_factory(sender.clone());
    let list_view = ListView::builder()
        .model(&selection_model)
        .factory(&factory)
        .margin_start(12)
        .margin_end(12)
        .margin_top(12)
        .margin_bottom(12)
        .show_separators(true)
        .build();
    list_view.connect_activate({
        let sender = sender.clone();
        move |lv: &ListView, _pos: u32| {
            let selection_model = lv.model().unwrap();
            let single_selection =
                selection_model.downcast_ref::<SingleSelection>().unwrap();
            let list_item = single_selection.selected_item().unwrap();
            let commit_item = list_item.downcast_ref::<CommitItem>().unwrap();
            let oid = commit_item.imp().commit.borrow().oid;
            sender
                .send_blocking(crate::Event::ShowOid(oid, None))
                .expect("cant send through sender");
        }
    });
    list_view
}

pub fn get_commit_list(list_view: &ListView) -> CommitList {
    let selection_model = list_view.model().unwrap();
    let single_selection =
        selection_model.downcast_ref::<SingleSelection>().unwrap();
    let list_model = single_selection.model().unwrap();
    let commit_list = list_model.downcast_ref::<CommitList>().unwrap();
    commit_list.to_owned()
}

pub fn headerbar_factory(
    list_view: &ListView,
    branch_name: String,
    window: &impl IsA<Widget>,
    sender: Sender<crate::Event>,
    repo_path: PathBuf,
) -> HeaderBar {
    let entry = SearchEntry::builder()
        .search_delay(300)
        .width_chars(22)
        .placeholder_text("hit s for search")
        .build();
    entry.connect_stop_search(|e| {
        e.stop_signal_emission_by_name("stop-search");
    });
    let commit_list = get_commit_list(list_view);

    let search = SearchBar::builder()
        .tooltip_text("search commits")
        .search_mode_enabled(true)
        .visible(true)
        .show_close_button(false)
        .child(&entry)
        .build();
    let very_first_search = Rc::new(RefCell::new(true));
    let threshold = Rc::new(RefCell::new(String::from("")));
    entry.connect_search_changed(
        clone!(@weak commit_list, @weak list_view, @strong very_first_search, @weak entry, @strong repo_path => move |e| {
            let term = e.text().to_lowercase();
            if !term.is_empty() && term.len() < 3 {
                return;
            }
            if term.is_empty() {
                let selection_model = list_view.model().unwrap();
                let single_selection =
                    selection_model.downcast_ref::<SingleSelection>().unwrap();
                single_selection.set_can_unselect(true);
                if *very_first_search.borrow() {
                    very_first_search.replace(false);
                } else {
                    commit_list.reset_search();
                    single_selection.set_can_unselect(false);
                }
            } else {
                threshold.replace(term);
                glib::source::timeout_add_local(Duration::from_millis(200), {
                    let entry = entry.clone();
                    let repo_path = repo_path.clone();
                    let threshold = threshold.clone();
                    move || {
                        let term = entry.text().to_lowercase();
                        if term == *threshold.borrow() {
                            commit_list.search(term, repo_path.clone(), &list_view);
                        }
                        glib::ControlFlow::Break
                    }
                });
            }
        }),
    );
    let title = Label::builder()
        .margin_start(12)
        .use_markup(true)
        .label(format!(
            "Commits in <span color=\"#4a708b\">{}</span>",
            branch_name
        ))
        .build();
    let hb = HeaderBar::builder().build();
    hb.set_title_widget(Some(&search));
    hb.pack_start(&title);

    let cherry_pick_btn = Button::builder()
        .icon_name("emblem-shared-symbolic")
        .can_shrink(true)
        .tooltip_text("Cherry-pick")
        .sensitive(true)
        .use_underline(true)
        .build();
    cherry_pick_btn.connect_clicked({
        let sender = sender.clone();
        let path = repo_path.clone();
        let window = window.clone();
        let commit_list = commit_list.clone();
        move |_btn| {
            commit_list.cherry_pick(path.clone(), &window, sender.clone());
        }
    });

    let revert_btn = Button::builder()
        .icon_name("edit-undo-symbolic")
        .can_shrink(true)
        .tooltip_text("Revert")
        .sensitive(true)
        .use_underline(true)
        .build();
    revert_btn.connect_clicked({
        let sender = sender.clone();
        let path = repo_path.clone();
        let window = window.clone();
        let commit_list = commit_list.clone();
        move |_btn| {
            commit_list.revert(path.clone(), &window, sender.clone());
        }
    });

    hb.pack_end(&cherry_pick_btn);
    hb.pack_end(&revert_btn);

    let reset_btn = Button::builder()
        .label("Reset hard")
        .use_underline(true)
        .can_focus(false)
        .tooltip_text("Reset hard")
        .icon_name("software-update-urgent-symbolic")
        .can_shrink(true)
        .build();
    reset_btn.connect_clicked({
        // let sender = sender.clone();
        let window = window.clone();
        let repo_path = repo_path.clone();
        move |_| {
            commit_list.reset_hard(repo_path.clone(), &window, sender.clone());
        }
    });
    hb.pack_end(&reset_btn);
    hb
}

pub fn show_log_window(
    repo_path: PathBuf,
    app_window: &impl IsA<Gtk4Window>,
    // app_window: &ApplicationWindow,
    branch_name: String,
    main_sender: Sender<crate::Event>,
    start_oid: Option<Oid>,
) -> Window {
    // let (sender, receiver) = async_channel::unbounded();

    let window = Window::builder()
        //.application(&app_window.application().unwrap())// panic!
        .transient_for(app_window)
        .default_width(640)
        .default_height(480)
        .build();
    window.set_default_size(1280, 960);

    let list_view = listview_factory(main_sender.clone());

    let scroll = ScrolledWindow::new();

    // reached works with pagedown instead of overshot
    scroll.connect_edge_reached({
        let repo_path = repo_path.clone();
        move |scroll, position| {
            if position != PositionType::Bottom {
                return;
            }
            let list_view = scroll.child().unwrap();
            let list_view = list_view.downcast_ref::<ListView>().unwrap();
            let commit_list = get_commit_list(list_view);
            // when doing search, it need to reset search term count
            // because commits pull 1 by 1. it need to reset counter
            let (term, _) = commit_list.imp().search_term.take();
            commit_list.imp().search_term.replace((term, 0));
            commit_list.get_commits_inside(repo_path.clone(), None, list_view);
        }
    });
    scroll.set_child(Some(&list_view));

    let tb = ToolbarView::builder().content(&scroll).build();

    let hb = headerbar_factory(
        &list_view,
        branch_name,
        &window,
        main_sender.clone(),
        repo_path.clone(),
    );

    tb.add_top_bar(&hb);
    window.set_content(Some(&tb));

    let event_controller = EventControllerKey::new();
    event_controller.connect_key_pressed({
        let window = window.clone();
        let list_view = list_view.clone();
        let main_sender = main_sender.clone();
        let repo_path = repo_path.clone();
        move |_, key, _, modifier| {
            match (key, modifier) {
                (gdk::Key::w, gdk::ModifierType::CONTROL_MASK) => {
                    window.close();
                }
                (gdk::Key::Escape, _) => {
                    window.close();
                }
                (gdk::Key::s, _) => {
                    let search_bar = hb.title_widget().unwrap();
                    let search_bar =
                        search_bar.downcast_ref::<SearchBar>().unwrap();
                    let search_entry = search_bar.child().unwrap();
                    let search_entry =
                        search_entry.downcast_ref::<SearchEntry>().unwrap();
                    trace!("enter search");
                    search_entry.grab_focus();
                }
                (gdk::Key::x, _) => {
                    get_commit_list(&list_view).reset_hard(
                        repo_path.clone(),
                        &window,
                        main_sender.clone(),
                    );
                }
                (gdk::Key::a, _) => {
                    get_commit_list(&list_view).cherry_pick(
                        repo_path.clone(),
                        &window,
                        main_sender.clone(),
                    );
                }
                (gdk::Key::r, _) => {
                    get_commit_list(&list_view).revert(
                        repo_path.clone(),
                        &window,
                        main_sender.clone(),
                    );
                }
                (key, modifier) => {
                    trace!("key pressed {:?} {:?}", key, modifier);
                }
            }
            glib::Propagation::Proceed
        }
    });
    window.add_controller(event_controller);
    window.present();
    trace!("grab list focus");
    list_view.grab_focus();
    get_commit_list(&list_view).get_commits_inside(
        repo_path.clone(),
        start_oid,
        &list_view,
    );
    window
}
