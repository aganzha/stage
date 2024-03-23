use async_channel::Sender;
use git2::BranchType;
use glib::{clone, closure, types, Object};
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{
    gdk, gio, glib, pango, AlertDialog, Box, Button, CheckButton,
    EventControllerKey, Label, ListHeader, ListItem, ListScrollFlags,
    ListView, NoSelection, Orientation, PropertyExpression, ScrolledWindow,
    SectionModel, SelectionModel, SignalListItemFactory, SingleSelection,
    Spinner, StringList, StringObject, Widget,
};
use libadwaita::prelude::*;
use libadwaita::{ApplicationWindow, HeaderBar, ToolbarView, Window};
use log::{debug, error, info, log_enabled, trace};
use std::thread;
use std::time::Duration;

glib::wrapper! {
    pub struct BranchItem(ObjectSubclass<branch_item::BranchItem>);
}

mod branch_item {
    use glib::Properties;
    use gtk4::glib;
    use gtk4::prelude::*;
    use gtk4::subclass::prelude::*;
    use std::cell::RefCell;

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::BranchItem)]
    pub struct BranchItem {
        pub branch: RefCell<crate::BranchData>,

        #[property(get, set)]
        pub initial_focus: RefCell<bool>,

        #[property(get, set)]
        pub progress: RefCell<bool>,

        #[property(get, set)]
        pub no_progress: RefCell<bool>,

        #[property(get, set)]
        pub is_head: RefCell<bool>,

        #[property(get, set)]
        pub ref_kind: RefCell<String>,

        #[property(get, set)]
        pub title: RefCell<String>,

        #[property(get, set)]
        pub last_commit: RefCell<String>,

        #[property(get, set)]
        pub dt: RefCell<String>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for BranchItem {
        const NAME: &'static str = "StageBranchItem";
        type Type = super::BranchItem;
    }
    #[glib::derived_properties]
    impl ObjectImpl for BranchItem {}
}

impl BranchItem {
    pub fn new(branch: crate::BranchData) -> Self {
        let ref_kind = {
            match branch.branch_type {
                BranchType::Local => String::from("Branches"),
                BranchType::Remote => String::from("Remote"),
            }
        };
        let ob = Object::builder::<BranchItem>()
            .property("is-head", branch.is_head)
            .property("progress", false)
            .property("no-progress", true)
            .property("ref-kind", ref_kind)
            .property(
                "title",
                format!("<span color=\"#4a708b\">{}</span>", &branch.name),
            )
            .property("last-commit", &branch.commit_string)
            .property("dt", branch.commit_dt.to_string())
            .property("initial-focus", false)
            .build();
        ob.imp().branch.replace(branch);
        ob
    }
}

glib::wrapper! {
    pub struct BranchList(ObjectSubclass<branch_list::BranchList>)
        @implements gio::ListModel, SectionModel;
}

mod branch_list {
    use crate::debug;
    use glib::Properties;
    use gtk4::gio;
    use gtk4::glib;
    use gtk4::prelude::*;
    use gtk4::subclass::prelude::*;
    use std::cell::RefCell;

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::BranchList)]
    pub struct BranchList {
        pub list: RefCell<Vec<super::BranchItem>>,
        pub remote_start_pos: RefCell<Option<u32>>,

        #[property(get, set)]
        pub proxyselected: RefCell<u32>,

        #[property(get, set)]
        pub current: RefCell<u32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for BranchList {
        const NAME: &'static str = "StageBranchList";
        type Type = super::BranchList;
        type ParentType = glib::Object;
        type Interfaces = (gio::ListModel, gtk4::SectionModel);
    }

    #[glib::derived_properties]
    impl ObjectImpl for BranchList {}

    impl ListModelImpl for BranchList {
        fn item_type(&self) -> glib::Type {
            super::BranchItem::static_type()
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
            // ??? clone ???
            return Some(list[position as usize].clone().into());
        }
    }

    impl SectionModelImpl for BranchList {
        fn section(&self, position: u32) -> (u32, u32) {
            if let Some(pos) = *self.remote_start_pos.borrow() {
                if position <= pos {
                    return (0, pos);
                } else {
                    return (pos, self.list.borrow().len() as u32);
                }
            }
            (0, self.list.borrow().len() as u32)
        }
    }
}

impl BranchList {
    pub fn new(sender: Sender<crate::Event>) -> Self {
        Object::builder().build()
    }

    pub fn make_list(&self, repo_path: std::ffi::OsString) {
        glib::spawn_future_local({
            clone!(@weak self as branch_list => async move {
                let branches: Vec<crate::BranchData> = gio::spawn_blocking(move || {
                    crate::get_refs(repo_path)
                }).await.expect("Task needs to finish successfully.");

                let items: Vec<BranchItem> = branches.into_iter()
                    .map(|branch| BranchItem::new(branch))
                    .collect();

                let le = items.len() as u32;
                let mut pos = 0;
                let mut remote_start_pos: Option<u32> = None;
                let mut selected = 0;
                for item in items {
                    if remote_start_pos.is_none() && item.imp().branch.borrow().branch_type == BranchType::Remote {
                        remote_start_pos.replace(pos as u32);
                    }
                    if item.imp().branch.borrow().is_head {
                        selected = pos;
                        item.set_initial_focus(true)
                    }
                    branch_list.imp().list.borrow_mut().push(item);
                    pos += 1;
                }
                branch_list.imp().remote_start_pos.replace(remote_start_pos);
                branch_list.items_changed(0, 0, le);
                // works via bind to single_selection selected
                branch_list.set_proxyselected(selected);
                branch_list.set_current(selected);
            })
        });
    }
    
    pub fn checkout(
        &self,
        repo_path: std::ffi::OsString,
        selected_item: &BranchItem,
        current_item: &BranchItem, // TODO refactor. get this item from branch_list itself
        window: &Window,
        sender: Sender<crate::Event>,
    ) {
        let branch_data = selected_item.imp().branch.borrow();
        let name = branch_data.refname.clone();
        let oid = branch_data.oid.clone();

        glib::spawn_future_local({
            clone!(@weak self as branch_list, @weak window as window, @weak selected_item, @weak current_item => async move {
                let result = gio::spawn_blocking(move || {
                    crate::checkout(repo_path, oid, &name, sender)
                }).await;
                let mut err_message = String::from("git error");
                if let Ok(git_result) = result {
                    selected_item.set_progress(false);
                    match git_result {
                        Ok(_) => {
                            selected_item.set_is_head(true);
                            selected_item.set_no_progress(true);
                            current_item.set_is_head(false);
                            branch_list.set_current(branch_list.proxyselected());
                            return;
                        }
                        Err(err) => err_message = err
                    }
                }
                selected_item.set_no_progress(true);
                crate::display_error(&window, &err_message);
            })
        });
    }

    pub fn cherry_pick(
        &self,
        repo_path: std::ffi::OsString,
        window: &Window,
        sender: Sender<crate::Event>,
    ) {
        glib::spawn_future_local({
            clone!(@weak self as branch_list, @weak window as window => async move {
                let pos = branch_list.proxyselected();
                let item = branch_list.item(pos).unwrap();
                let branch_item = item.downcast_ref::<BranchItem>().unwrap();
                let branch_data = branch_item.imp().branch.borrow().clone();
                let result = gio::spawn_blocking(move || {
                    crate::cherry_pick(repo_path, branch_data, sender)
                }).await;
                let mut err_message = String::from("git error");
                if let Ok(git_result) = result {
                    match git_result {
                        Ok(branch_data) => {
                            debug!("oooooooooooooooooou {:?}", branch_data);
                            let current_item = branch_list.item(branch_list.current()).unwrap();
                            let branch_item = item.downcast_ref::<BranchItem>().unwrap();
                            branch_item.imp().branch.replace(branch_data);
                            return;
                        }
                        Err(err) => err_message = err
                    }
                }
                crate::display_error(&window, &err_message);
            })
        });
    }

    pub fn kill_branch(
        &self,
        repo_path: std::ffi::OsString,
        window: &Window,
        sender: Sender<crate::Event>,
    ) {
        glib::spawn_future_local({
            clone!(@weak self as branch_list, @weak window as window => async move {
                let pos = branch_list.proxyselected();
                let item = branch_list.item(pos).unwrap();
                let branch_item = item.downcast_ref::<BranchItem>().unwrap();
                let branch_data = branch_item.imp().branch.borrow().clone();
                let kind = branch_data.branch_type;
                let result = gio::spawn_blocking(move || {
                    crate::kill_branch(repo_path, branch_data, sender)
                }).await;
                let mut err_message = String::from("git error");
                if let Ok(git_result) = result {
                    match git_result {
                        Ok(_) => {
                            {
                                // put borrow in block
                                branch_list.imp().list.borrow_mut().remove(pos as usize);
                                if kind == BranchType::Local {
                                    let mut pos = branch_list.imp().remote_start_pos.borrow_mut();
                                    if let Some(mut rem_pos) = *pos {
                                        rem_pos -= 1;
                                        pos.replace(rem_pos);
                                        trace!("branches.replace rem pos {:?} {:?}", rem_pos, pos);
                                    }
                                }
                            }
                            let shifted_item = branch_list.item(pos);
                            trace!("branches. removed item at pos {:?}", pos);
                            let mut new_pos = pos;
                            if let Some(item) = shifted_item {
                                trace!("branches.shift item");
                                // next item in list will shift to this position
                                // and must get focus
                                let branch_item = item.downcast_ref::<BranchItem>().unwrap();
                                branch_item.set_initial_focus(true);
                                // if not select new_pos there will be panic in transform_to
                                // there will be no value (no item) in selected-item
                                // during items_changed
                                branch_list.set_proxyselected(0);
                            } else {
                                new_pos = {
                                    if pos > 1 {
                                        pos - 1
                                    } else {
                                        0
                                    }
                                };
                                trace!("branches.got last item. decrement pos {:?}", new_pos);
                                let prev_item = branch_list.item(new_pos).unwrap();
                                let branch_item = prev_item.downcast_ref::<BranchItem>().unwrap();
                                branch_item.set_initial_focus(true);
                                branch_list.set_proxyselected(new_pos);
                            }
                            branch_list.items_changed(pos, 1, 0);
                            // restore selected position to next one
                            // will will get focus
                            // when delete LAST list item, next expr has no effect:
                            // there will be item with overflown position
                            // connect_selected_notify and cursor will jump
                            // to first position
                            branch_list.set_proxyselected(new_pos);
                            return;
                        }
                        Err(err) => err_message = err
                    }
                }
                crate::display_error(&window, &err_message);
            })
        });
    }

    pub fn create_branch(
        &self,
        repo_path: std::ffi::OsString,
        new_branch_name: String,
        window: &Window,
        branch_sender: Sender<Event>,
        sender: Sender<crate::Event>,
    ) {
        glib::spawn_future_local({
            clone!(@weak self as branch_list, @weak window as window => async move {
                let item = branch_list.item(branch_list.proxyselected()).unwrap();
                let branch_item = item.downcast_ref::<BranchItem>().unwrap();
                let branch_data = branch_item.imp().branch.borrow().clone();
                let result = gio::spawn_blocking(move || {
                    crate::create_branch(repo_path, new_branch_name, branch_data, sender)
                }).await;
                let mut err_message = String::from("git error");
                if let Ok(git_result) = result {
                    match git_result {
                        Ok(branch_data) => {
                            // branch_item.set_is_head(false);
                            let new_item = BranchItem::new(branch_data);
                            trace!("branches.just created new item {:?}", new_item.is_head());
                            {
                                // put borrow in block
                                branch_list.imp().list.borrow_mut().insert(0, new_item);
                                let mut pos = branch_list.imp().remote_start_pos.borrow_mut();
                                if let Some(mut rem_pos) = *pos {
                                    rem_pos += 1;
                                    pos.replace(rem_pos);
                                    trace!("branches. replace rem pos {:?} {:?}", rem_pos, pos);
                                }
                            }
                            branch_list.items_changed(0, 0, 1);
                            branch_list.set_proxyselected(0);
                            branch_sender.send_blocking(Event::Scroll(0))
                                .expect("Could not send through channel");
                            // TODO! it must be activated, not only selected!
                            return;
                        }
                        Err(err) => err_message = err
                    }
                }
                crate::display_error(&window, &err_message);
            })
        });
    }
}

pub fn make_header_factory() -> SignalListItemFactory {
    let section_title = std::cell::RefCell::new(String::from("Branches"));
    let header_factory = SignalListItemFactory::new();
    header_factory.connect_setup(move |_, list_header| {
        let label = Label::new(Some(&*section_title.borrow()));
        let list_header = list_header
            .downcast_ref::<ListHeader>()
            .expect("Needs to be ListHeader");
        list_header.set_child(Some(&label));
        section_title.replace(String::from("Remotes"));
        // does not work. it is always git first BranchItem
        // why???
        // list_header.connect_item_notify(move |lh| {
        //     let ob = lh.item().unwrap();
        //     let item: &BranchItem = ob
        //         .downcast_ref::<BranchItem>()
        //         .unwrap();
        //     // let title = match item.imp().branch.borrow().branch_type {
        //     //     BranchType::Local => "Branches",
        //     //     BranchType::Remote => "Remote"
        //     // };
        //     // label.set_label(title);
        // });
        // does not work also
        // let item = list_header
        //     .property_expression("item");
        // item.chain_property::<BranchItem>("ref-kind")
        //     .bind(&label, "label", Widget::NONE);
    });
    header_factory
}

pub fn make_item_factory() -> SignalListItemFactory {
    let factory = SignalListItemFactory::new();
    factory.connect_setup(move |_, list_item| {
        let fake_btn = CheckButton::new();
        let btn = CheckButton::new();
        btn.set_group(Some(&fake_btn));
        btn.set_sensitive(false);
        let spinner = Spinner::new();
        spinner.set_visible(false);
        // spinner.set_spinning(true);
        let label_title = Label::builder()
            .label("")
            .lines(1)
            .single_line_mode(true)
            .xalign(0.0)
            .width_chars(24)
            .max_width_chars(24)
            .ellipsize(pango::EllipsizeMode::End)
            //.selectable(true)
            .use_markup(true)
            .can_focus(true)
            .can_target(true)
            .build();
        let label_commit = Label::builder()
            .label("")
            .lines(1)
            .single_line_mode(true)
            .xalign(0.0)
            .width_chars(36)
            .max_width_chars(36)
            .ellipsize(pango::EllipsizeMode::End)
            //.selectable(true)
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
            //.selectable(true)
            .use_markup(true)
            .can_focus(true)
            .can_target(true)
            .build();

        let bx = Box::builder()
            .orientation(Orientation::Horizontal)
            .margin_top(2)
            .margin_bottom(2)
            .margin_start(2)
            .margin_end(2)
            .spacing(12)
            .can_focus(true)
            .focusable(true)
            .build();
        bx.append(&btn);
        bx.append(&spinner);
        bx.append(&label_title);
        bx.append(&label_commit);
        bx.append(&label_dt);

        let list_item = list_item
            .downcast_ref::<ListItem>()
            .expect("Needs to be ListItem");
        list_item.set_child(Some(&bx));
        list_item.set_selectable(true);
        list_item.set_activatable(true);
        list_item.set_focusable(true);
        list_item.connect_selected_notify(|li: &ListItem| {
            // grab focus only once on list init
            if let Some(item) = li.item() {
                let branch_item = item.downcast_ref::<BranchItem>().unwrap();
                trace!(
                    "item in connect selected {:?} {:?} {:?}",
                    branch_item.title(),
                    branch_item.initial_focus(),
                    li.position()
                );
                if branch_item.initial_focus() {
                    li.child().unwrap().grab_focus();
                    branch_item.set_initial_focus(false)
                }
            } else {
                trace!(
                    "branches. no item in connect_selected_notify {:?}",
                    li.position()
                );
            }
        });

        let item = list_item.property_expression("item");

        item.chain_property::<BranchItem>("is_head").bind(
            &btn,
            "active",
            Widget::NONE,
        );
        item.chain_property::<BranchItem>("no-progress").bind(
            &btn,
            "visible",
            Widget::NONE,
        );
        item.chain_property::<BranchItem>("progress").bind(
            &spinner,
            "visible",
            Widget::NONE,
        );
        item.chain_property::<BranchItem>("progress").bind(
            &spinner,
            "spinning",
            Widget::NONE,
        );
        item.chain_property::<BranchItem>("title").bind(
            &label_title,
            "label",
            Widget::NONE,
        );

        item.chain_property::<BranchItem>("last-commit").bind(
            &label_commit,
            "label",
            Widget::NONE,
        );

        item.chain_property::<BranchItem>("dt").bind(
            &label_dt,
            "label",
            Widget::NONE,
        );
    });

    factory
}

pub fn make_list_view(
    repo_path: std::ffi::OsString,
    sender: Sender<crate::Event>,
) -> ListView {
    let header_factory = make_header_factory();
    let factory = make_item_factory();

    let branch_list = BranchList::new(sender.clone());

    let selection_model = SingleSelection::new(Some(branch_list));
    selection_model.set_autoselect(false);

    let model = selection_model.model().unwrap();
    let bind =
        selection_model.bind_property("selected", &model, "proxyselected");
    let _ = bind.bidirectional().build();

    let branch_list = model.downcast_ref::<BranchList>().unwrap();
    branch_list.make_list(repo_path.clone());

    let list_view = ListView::builder()
        .model(&selection_model)
        .factory(&factory)
        .header_factory(&header_factory)
        .margin_start(12)
        .margin_end(12)
        .margin_top(12)
        .margin_bottom(12)
        .show_separators(true)
        .build();

    list_view.connect_activate(move |lv: &ListView, pos: u32| {
        let selection_model = lv.model().unwrap();
        let single_selection =
            selection_model.downcast_ref::<SingleSelection>().unwrap();
        let list_model = single_selection.model().unwrap();
        let branch_list = list_model.downcast_ref::<BranchList>().unwrap();

        let item_ob = selection_model.item(pos);
        let mut current_item: Option<&BranchItem> = None;
        if let Some(item) = item_ob {
            let list = branch_list.imp().list.borrow();
            let activated_branch_item =
                item.downcast_ref::<BranchItem>().unwrap();
            if activated_branch_item.is_head() {
                return;
            }
            for branch_item in list.iter() {
                if branch_item.is_head() {
                    current_item.replace(branch_item);
                }
                branch_item.set_progress(false);
                branch_item.set_no_progress(true);
            }
            activated_branch_item.set_progress(true);
            activated_branch_item.set_no_progress(false);
            let root = lv.root().unwrap();
            let window = root.downcast_ref::<Window>().unwrap();
            trace!(
                "branches checkout! {:?} {:?}",
                single_selection.selected(),
                branch_list.proxyselected()
            );
            branch_list.checkout(
                repo_path.clone(),
                &activated_branch_item,
                // got panic here!
                current_item.unwrap(),
                &window,
                sender.clone(),
            );
        }
    });
    list_view.add_css_class("stage");
    list_view
}

pub fn make_headerbar(
    repo_path: std::ffi::OsString,
    list_view: &ListView,
    sender: Sender<Event>,
) -> HeaderBar {
    let hb = HeaderBar::builder().build();
    let lbl = Label::builder()
        .label("branches")
        .single_line_mode(true)
        .build();
    let new_btn = Button::builder()
        .label("N")
        .can_shrink(true)
        .tooltip_text("New")
        .sensitive(true)
        .use_underline(true)
        // .action_name("branches.new")
        .build();
    new_btn.connect_clicked({
        let sender = sender.clone();
        move |_| {
            sender
                .send_blocking(Event::NewBranchRequest)
                .expect("Could not send through channel");
        }
    });
    let kill_btn = Button::builder()
        .label("K")
        .use_underline(true)
        .tooltip_text("Kill")
        .sensitive(false)
        .can_shrink(true)
        //.action_name("branches.kill")
        .build();
    let selection_model = list_view.model().unwrap();
    let single_selection =
        selection_model.downcast_ref::<SingleSelection>().unwrap();

    let _ = single_selection
        .bind_property("selected-item", &kill_btn, "sensitive")
        .transform_to(move |_, item: BranchItem| Some(!item.is_head()))
        .build();
    kill_btn.connect_clicked({
        let sender = sender.clone();
        move |_| {
            sender
                .send_blocking(Event::KillRequest)
                .expect("Could not send through channel");
        }
    });
    let merge_btn = Button::builder()
        .label("M")
        .use_underline(true)
        .tooltip_text("Merge")
        .sensitive(false)
        .can_shrink(true)
        //.action_name("branches.merge")
        .build();
    let _ = single_selection
        .bind_property("selected-item", &merge_btn, "sensitive")
        .transform_to(move |_, item: BranchItem| Some(!item.is_head()))
        .build();
    merge_btn.connect_clicked({
        let sender = sender.clone();
        move |_| {
            sender
                .send_blocking(Event::MergeRequest)
                .expect("Could not send through channel");
        }
    });

    hb.set_title_widget(Some(&lbl));
    hb.pack_end(&new_btn);
    hb.pack_end(&kill_btn);
    hb.pack_end(&merge_btn);
    hb.set_show_end_title_buttons(true);
    hb.set_show_back_button(true);
    hb
}

pub enum Event {
    NewBranchRequest,
    NewBranch(String),
    Scroll(u32),
    KillRequest,
    MergeRequest,
    CherryPickRequest,
}

pub fn branches_in_use(
    list_view: &ListView,
) -> (crate::BranchData, crate::BranchData) {
    let selection_model = list_view.model().unwrap();
    let single_selection =
        selection_model.downcast_ref::<SingleSelection>().unwrap();
    let list_model = single_selection.model().unwrap();
    let branch_list = list_model.downcast_ref::<BranchList>().unwrap();

    let selected_item = branch_list.item(branch_list.proxyselected()).unwrap();
    let selected_branch_item =
        selected_item.downcast_ref::<BranchItem>().unwrap();
    let selected_branch_data =
        selected_branch_item.imp().branch.borrow().clone();

    let current_item = branch_list.item(branch_list.current()).unwrap();
    let current_branch_item =
        current_item.downcast_ref::<BranchItem>().unwrap();
    let current_branch_data =
        current_branch_item.imp().branch.borrow().clone();
    (current_branch_data, selected_branch_data)
}

pub fn show_branches_window(
    repo_path: std::ffi::OsString,
    app_window: &ApplicationWindow,
    main_sender: Sender<crate::Event>,
) {
    let (sender, receiver) = async_channel::unbounded();

    let window = Window::builder()
        .application(&app_window.application().unwrap())
        .transient_for(app_window)
        .default_width(640)
        .default_height(480)
        .build();
    window.set_default_size(1280, 960);

    let scroll = ScrolledWindow::new();

    let list_view = make_list_view(repo_path.clone(), main_sender.clone());

    let hb = make_headerbar(repo_path.clone(), &list_view, sender.clone());

    scroll.set_child(Some(&list_view));

    let tb = ToolbarView::builder().content(&scroll).build();
    tb.add_top_bar(&hb);

    window.set_content(Some(&tb));

    let event_controller = EventControllerKey::new();
    event_controller.connect_key_pressed({
        let window = window.clone();
        let sender = sender.clone();
        move |_, key, _, modifier| {
            match (key, modifier) {
                (gdk::Key::w, gdk::ModifierType::CONTROL_MASK) => {
                    window.close();
                }
                (gdk::Key::n, _) => {
                    sender
                        .send_blocking(Event::NewBranchRequest)
                        .expect("Could not send through channel");
                }
                (gdk::Key::k, _) => {
                    sender
                        .send_blocking(Event::KillRequest)
                        .expect("Could not send through channel");
                }
                (gdk::Key::m, _) => {
                    sender
                        .send_blocking(Event::MergeRequest)
                        .expect("Could not send through channel");
                }
                (gdk::Key::a, _) => {
                    sender
                        .send_blocking(Event::CherryPickRequest)
                        .expect("Could not send through channel");
                }
                _ => {}
            }
            glib::Propagation::Proceed
        }
    });
    window.add_controller(event_controller);

    window.present();
    list_view.grab_focus();

    glib::spawn_future_local(async move {
        while let Ok(event) = receiver.recv().await {
            match event {
                Event::NewBranchRequest => {
                    let (current_branch, _) = branches_in_use(&list_view);
                    info!("branches. new branch request");
                    crate::get_new_branch_name(
                        &window,
                        &current_branch,
                        sender.clone(),
                    );
                }
                Event::NewBranch(new_branch_name) => {
                    info!("branches. got new branch name");
                    let selection_model = list_view.model().unwrap();
                    let single_selection = selection_model
                        .downcast_ref::<SingleSelection>()
                        .unwrap();
                    let list_model = single_selection.model().unwrap();
                    let branch_list =
                        list_model.downcast_ref::<BranchList>().unwrap();
                    branch_list.create_branch(
                        repo_path.clone(),
                        new_branch_name,
                        &window,
                        sender.clone(),
                        main_sender.clone(),
                    );
                }
                Event::Scroll(pos) => {
                    info!("branches. scroll {:?}", pos);
                    list_view.scroll_to(pos, ListScrollFlags::empty(), None);
                }
                Event::KillRequest => {
                    info!("branches. kill request");
                    let selection_model = list_view.model().unwrap();
                    let single_selection = selection_model
                        .downcast_ref::<SingleSelection>()
                        .unwrap();
                    let item = single_selection.selected_item();
                    if let Some(item) = item {
                        if !item
                            .downcast_ref::<BranchItem>()
                            .unwrap()
                            .is_head()
                        {
                            let list_model = single_selection.model().unwrap();
                            let branch_list = list_model
                                .downcast_ref::<BranchList>()
                                .unwrap();
                            branch_list.kill_branch(
                                repo_path.clone(),
                                &window,
                                main_sender.clone(),
                            );
                        }
                    }
                }
                Event::MergeRequest => {
                    info!("branches. merge request");
                }
                Event::CherryPickRequest => {
                    info!("branches. cherry-pick request");
                    let (current_branch, selected_branch) =
                        branches_in_use(&list_view);
                    debug!(
                        "==========================> {:?} {:?}",
                        current_branch, selected_branch
                    );
                    let btns = vec!["Cancel", "Cherry-pick"];
                    let alert = AlertDialog::builder()
                        .buttons(btns)
                        .message("Cherry picking")
                        .detail(format!(
                            "Cherry picing commit {:?} from branch {:?} onto branch {:?}",
                            selected_branch.commit_string, selected_branch.name, current_branch.name
                        ))
                        .build();
                    let selection_model = list_view.model().unwrap();
                    let single_selection = selection_model
                        .downcast_ref::<SingleSelection>()
                        .unwrap();
                    let list_model = single_selection.model().unwrap();
                    let branch_list =
                        list_model.downcast_ref::<BranchList>().unwrap();
                    alert.choose(Some(&window), None::<&gio::Cancellable>, {
                        let path = repo_path.clone();
                        let window = window.clone();
                        let sender = main_sender.clone();
                        clone!(@weak branch_list => move |result| {
                            debug!("meeeeeeeeeeeeeeeeee {:?}", result);
                            if let Ok(ind) = result {
                                if ind == 1 {
                                    branch_list.cherry_pick(
                                        path,
                                        &window,
                                        sender
                                    )
                                }
                            }
                        })
                    });
                }
            }
        }
    });
}
