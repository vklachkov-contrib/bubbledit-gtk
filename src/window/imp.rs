// SPDX-License-Identifier: GPL-2.0-or-later
//
// bubbledit-gtk
// A tool to manipulate MAME image files of Intel's bubble memories
// Copyright (C) 2026 F. Ulivi <fulivi at big "G" mail>
//
// This program is free software; you can redistribute it and/or
// modify it under the terms of the GNU General Public License
// as published by the Free Software Foundation; either version 2
// of the License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see
// <http://www.gnu.org/licenses/>.

use crate::bubble;
use glib::clone;
use glib::subclass::InitializingObject;
use gtk::builders::AlertDialogBuilder;
use gtk::gio;
use gtk::gio::prelude::FileExt;
use gtk::gio::{Cancellable, File, ListStore};
use gtk::glib::{Error, Propagation};
use gtk::prelude::{
    ButtonExt, Cast, CheckButtonExt, EditableExt, GtkWindowExt, StaticType, WidgetExt,
};
use gtk::subclass::prelude::*;
use gtk::{
    AboutDialog, AlertDialog, Button, CallbackAction, CheckButton, CompositeTemplate, Entry,
    FileDialog, FileFilter, Label, Notebook, Shortcut, ShortcutController, ShortcutTrigger,
    SpinButton, Widget, glib,
};
use std::cell::RefCell;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::rc::Rc;

#[derive(Default)]
struct Dati {
    mem: bubble::Memory,
    path: PathBuf,
    leaving: bool,
}

// Object holding the state
#[derive(CompositeTemplate, Default)]
#[template(resource = "/bubbledit-gtk/window.ui")]
pub struct Window {
    #[template_child]
    pub title: TemplateChild<Label>,
    #[template_child]
    pub notebook: TemplateChild<Notebook>,
    #[template_child]
    pub bl_page: TemplateChild<Widget>,
    #[template_child]
    pub log_page: TemplateChild<Widget>,
    #[template_child]
    pub phy_page: TemplateChild<Widget>,
    #[template_child]
    pub bl_bootloop: TemplateChild<Entry>,
    #[template_child]
    pub bl_ch_a: TemplateChild<Entry>,
    #[template_child]
    pub bl_ch_b: TemplateChild<Entry>,
    #[template_child]
    pub bl_ch_a_ones: TemplateChild<Label>,
    #[template_child]
    pub bl_ch_b_ones: TemplateChild<Label>,
    #[template_child]
    pub log_address: TemplateChild<Label>,
    #[template_child]
    pub log_spin: TemplateChild<SpinButton>,
    #[template_child]
    pub log_ascii: TemplateChild<Label>,
    #[template_child]
    pub log_data: TemplateChild<Entry>,
    #[template_child]
    pub log_ch_a_code: TemplateChild<Entry>,
    #[template_child]
    pub log_ch_b_code: TemplateChild<Entry>,
    #[template_child]
    pub log_ch_a_correct: TemplateChild<Button>,
    #[template_child]
    pub log_ch_b_correct: TemplateChild<Button>,
    #[template_child]
    pub phy_address: TemplateChild<Label>,
    #[template_child]
    pub phy_spin: TemplateChild<SpinButton>,
    #[template_child]
    pub phy_ch_a: TemplateChild<Entry>,
    #[template_child]
    pub phy_ch_b: TemplateChild<Entry>,
    #[template_child]
    pub phy_skip: TemplateChild<CheckButton>,

    dati: Rc<RefCell<Dati>>,
}

// The central trait for subclassing a GObject
#[glib::object_subclass]
impl ObjectSubclass for Window {
    // `NAME` needs to match `class` attribute of template
    const NAME: &'static str = "MyGtkAppWindow";
    type Type = super::Window;
    type ParentType = gtk::ApplicationWindow;

    fn class_init(klass: &mut Self::Class) {
        klass.bind_template();
        klass.bind_template_callbacks();
    }

    fn instance_init(obj: &InitializingObject<Self>) {
        obj.init_template();
    }
}

// Trait shared by all GObjects
impl ObjectImpl for Window {
    fn constructed(&self) {
        self.parent_constructed();
        self.obj().setup_actions();
        self.update_curr_page();
        let sct = ShortcutTrigger::parse_string("Escape");
        let sca = CallbackAction::new(clone!(
            #[weak(rename_to = win)]
            self,
            #[upgrade_or_panic]
            move |_, _| {
                win.update_curr_page();
                Propagation::Proceed
            }
        ));
        if let Some(scto) = sct {
            let sc = Shortcut::builder().trigger(&scto).action(&sca).build();
            let scc = ShortcutController::new();
            scc.add_shortcut(sc);
            self.notebook.add_controller(scc);
        }
    }
}

// Trait shared by all widgets
impl WidgetImpl for Window {}

// Trait shared by all windows
impl WindowImpl for Window {
    fn close_request(&self) -> Propagation {
        let quit: bool;
        {
            let d = self.dati.borrow();
            quit = d.leaving || !d.mem.is_dirty();
        }
        if !quit {
            self.check_saving_needed(
                false,
                clone!(
                    #[weak(rename_to = win)]
                    self,
                    move || win.quit()
                ),
            );
            Propagation::Stop
        } else {
            Propagation::Proceed
        }
    }
}

// Trait shared by all application windows
impl ApplicationWindowImpl for Window {}

#[gtk::template_callbacks]
impl Window {
    fn quit(&self) {
        self.dati.borrow_mut().leaving = true;
        self.obj().close();
    }
    pub fn open_file(&self, f: gio::File) {
        self.file_load_3(Ok(f));
    }
    fn set_title(&self) {
        let d = self.dati.borrow();
        let ch = if d.mem.is_dirty() { '*' } else { ' ' };
        if d.path.as_os_str().is_empty() {
            self.title.set_label(&format!("{}Unnamed", ch));
        } else {
            if let Some(filename) = d.path.file_name() {
                if let Some(filename_s) = filename.to_str() {
                    self.title
                        .set_label(&format!("<b>{}{}</b>", ch, filename_s));
                }
            }
        }
    }
    fn fill_bl_page(&self) {
        let d = self.dati.borrow();
        let ch_a = d.mem.get_ch(bubble::Channel::ChannelA).get_bl();
        let s_a = ch_a.to_string();
        self.bl_ch_a.set_text(&s_a);
        let s_ch_a = format!("{:3} 1s", ch_a.ones());
        self.bl_ch_a_ones.set_label(&s_ch_a);
        let ch_b = d.mem.get_ch(bubble::Channel::ChannelB).get_bl();
        let s_b = ch_b.to_string();
        self.bl_ch_b.set_text(&s_b);
        let s_ch_b = format!("{:3} 1s", ch_b.ones());
        self.bl_ch_b_ones.set_label(&s_ch_b);
        let mut bl = bubble::Bootloop::new();
        bl.set_from_channel(bubble::Channel::ChannelA, ch_a);
        bl.set_from_channel(bubble::Channel::ChannelB, ch_b);
        let s_bl = bl.to_string();
        self.bl_bootloop.set_text(&s_bl);
    }
    fn corr_button_style(res: bubble::FireResult) -> (&'static str, &'static str) {
        match res {
            bubble::FireResult::Ok => ("Ok", "code_ok"),
            bubble::FireResult::Correctable => ("Correctable", "code_corr"),
            bubble::FireResult::Uncorrectable => ("Uncorrectable", "code_uncorr"),
            _ => ("???", ""),
        }
    }
    fn fill_log_page(&self) {
        let mut d = self.dati.borrow_mut();
        let la = self.log_spin.value_as_int() as bubble::Address;
        let (res_a, res_b) = d.mem.get_la_page(la, true);
        let s = d.mem.payload_to_string();
        self.log_data.set_text(&s);
        let s_asc = d.mem.payload_to_ascii();
        self.log_ascii.set_label(&s_asc);
        let s_code_a = d.mem.code_to_string(bubble::Channel::ChannelA);
        self.log_ch_a_code.set_text(&s_code_a);
        let s_code_b = d.mem.code_to_string(bubble::Channel::ChannelB);
        self.log_ch_b_code.set_text(&s_code_b);
        let pa = bubble::ChannelMemory::la_to_pa(la);
        let s_a = format!("LA {:03x} PA {:03x}", la, pa);
        self.log_address.set_label(&s_a);
        let (s_bt_a, class_bt_a) = Self::corr_button_style(res_a);
        self.log_ch_a_correct.set_label(s_bt_a);
        self.log_ch_a_correct.set_css_classes(&[class_bt_a]);
        let (s_bt_b, class_bt_b) = Self::corr_button_style(res_b);
        self.log_ch_b_correct.set_label(s_bt_b);
        self.log_ch_b_correct.set_css_classes(&[class_bt_b]);
    }
    fn fill_phy_page(&self) {
        let mut d = self.dati.borrow_mut();
        let pa = self.phy_spin.value_as_int() as bubble::Address;
        let skip = self.phy_skip.is_active();
        let s_ch_a = d.mem.page_pa_to_string(bubble::Channel::ChannelA, pa, skip);
        self.phy_ch_a.set_text(&s_ch_a);
        let s_ch_b = d.mem.page_pa_to_string(bubble::Channel::ChannelB, pa, skip);
        self.phy_ch_b.set_text(&s_ch_b);
        let (la, u) = bubble::ChannelMemory::pa_to_la(pa);
        let s_a = format!("LA {:03x}+{} PA {:03x}", la, if u == 0 { 0 } else { 2 }, pa);
        self.phy_address.set_label(&s_a);
    }
    fn update_page(&self, page: &Widget) {
        if *page == *self.bl_page {
            self.fill_bl_page();
        } else if *page == *self.log_page {
            self.fill_log_page();
        } else if *page == *self.phy_page {
            self.fill_phy_page();
        }
    }
    fn update_curr_page(&self) {
        let pg = self.notebook.current_page();
        let wgo = self.notebook.nth_page(pg);
        if let Some(wg) = wgo {
            self.update_page(&wg);
        }
        self.set_title();
    }
    #[template_callback]
    fn handle_switch_page(&self, new_page: &Widget) {
        if self.notebook.is_bound() {
            self.update_page(new_page);
        }
    }

    fn alert_msg(&self, msg: &str) {
        let alert = AlertDialog::builder().modal(true).message(msg).build();
        let c = self.obj();
        let app_window = Some(c.upcast_ref::<gtk::Window>());
        alert.show(app_window);
    }
    fn alert_no_parse(&self) {
        self.alert_msg("Can't parse!");
    }

    #[template_callback]
    fn handle_bl_activate(&self, _: &Entry) {
        self.check_saving_needed(
            false,
            clone!(
                #[weak(rename_to = win)]
                self,
                move || win.handle_bl_activate_2()
            ),
        );
    }
    fn handle_bl_activate_2(&self) {
        let res: bool;
        {
            let mut d = self.dati.borrow_mut();
            let s = self.bl_bootloop.text();
            let mut bl = bubble::Bootloop::new();
            res = bl.set_from_string(&s);
            if res {
                d.mem.set_ch_bl(
                    Some(&bl.get_channel(bubble::Channel::ChannelA)),
                    Some(&bl.get_channel(bubble::Channel::ChannelB)),
                );
                d.path = PathBuf::new();
            }
        }
        if res {
            self.update_curr_page();
        } else {
            self.alert_no_parse();
        }
    }
    #[template_callback]
    fn handle_bl_ch_a_activate(&self, _: &Entry) {
        self.check_saving_needed(
            false,
            clone!(
                #[weak(rename_to = win)]
                self,
                move || win.handle_bl_ch_a_activate_2()
            ),
        );
    }
    fn handle_bl_ch_a_activate_2(&self) {
        let res: bool;
        {
            let s = self.bl_ch_a.text();
            let mut cha = bubble::ChannelBootloop::new();
            res = cha.set_from_string(&s);
            if res {
                let mut d = self.dati.borrow_mut();
                d.mem.set_ch_bl(Some(&cha), None);
                d.path = PathBuf::new();
            }
        }
        if res {
            self.update_curr_page();
        } else {
            self.alert_no_parse();
        }
    }
    #[template_callback]
    fn handle_bl_ch_b_activate(&self, _: &Entry) {
        self.check_saving_needed(
            false,
            clone!(
                #[weak(rename_to = win)]
                self,
                move || win.handle_bl_ch_b_activate_2()
            ),
        );
    }
    fn handle_bl_ch_b_activate_2(&self) {
        let res: bool;
        {
            let s = self.bl_ch_b.text();
            let mut chb = bubble::ChannelBootloop::new();
            res = chb.set_from_string(&s);
            if res {
                let mut d = self.dati.borrow_mut();
                d.mem.set_ch_bl(None, Some(&chb));
                d.path = PathBuf::new();
            }
        }
        if res {
            self.update_curr_page();
        } else {
            self.alert_no_parse();
        }
    }
    #[template_callback]
    fn handle_bl_random_clicked(&self, _: &Button) {
        self.check_saving_needed(
            false,
            clone!(
                #[weak(rename_to = win)]
                self,
                move || win.handle_bl_random_clicked_2()
            ),
        );
    }
    fn handle_bl_random_clicked_2(&self) {
        let mut ch_a = bubble::ChannelBootloop::new();
        ch_a.set_random(139);
        let mut ch_b = bubble::ChannelBootloop::new();
        ch_b.set_random(139);
        {
            let mut d = self.dati.borrow_mut();
            d.mem.set_ch_bl(Some(&ch_a), Some(&ch_b));
            d.path = PathBuf::new();
        }
        self.update_curr_page();
    }

    #[template_callback]
    fn handle_log_la_changed(&self, _: &SpinButton) {
        self.update_curr_page();
    }

    #[template_callback]
    fn handle_log_data_activate(&self, _: &Entry) {
        let res: Result<(), ()>;
        {
            let s = self.log_data.text();
            let mut d = self.dati.borrow_mut();
            res = d.mem.payload_from_string(&s);
        }
        if res.is_ok() {
            self.update_curr_page();
        } else {
            self.alert_no_parse();
        }
    }

    #[template_callback]
    fn handle_ch_a_code_activate(&self, _: &Entry) {
        let res: Result<(), ()>;
        {
            let s = self.log_ch_a_code.text();
            let mut d = self.dati.borrow_mut();
            res = d.mem.code_from_string(bubble::Channel::ChannelA, &s);
        }
        if res.is_ok() {
            self.update_curr_page();
        } else {
            self.alert_no_parse();
        }
    }

    #[template_callback]
    fn handle_ch_b_code_activate(&self, _: &Entry) {
        let res: Result<(), ()>;
        {
            let s = self.log_ch_b_code.text();
            let mut d = self.dati.borrow_mut();
            res = d.mem.code_from_string(bubble::Channel::ChannelB, &s);
        }
        if res.is_ok() {
            self.update_curr_page();
        } else {
            self.alert_no_parse();
        }
    }

    #[template_callback]
    fn handle_log_regen_clicked(&self, _: &Button) {
        {
            let mut d = self.dati.borrow_mut();
            let _ = d.mem.regen_code();
        }
        self.update_curr_page();
    }

    #[template_callback]
    fn handle_ch_a_correct_clicked(&self, _: &Button) {
        {
            let mut d = self.dati.borrow_mut();
            let _ = d.mem.apply_correction(bubble::Channel::ChannelA);
        }
        self.update_curr_page();
    }

    #[template_callback]
    fn handle_ch_b_correct_clicked(&self, _: &Button) {
        {
            let mut d = self.dati.borrow_mut();
            let _ = d.mem.apply_correction(bubble::Channel::ChannelB);
        }
        self.update_curr_page();
    }
    #[template_callback]
    fn handle_phy_pa_changed(&self, _: &SpinButton) {
        self.update_curr_page();
    }
    #[template_callback]
    fn handle_phy_skip_toggled(&self, _: &CheckButton) {
        self.update_curr_page();
    }

    #[template_callback]
    fn handle_phy_ch_a_activate(&self, _: &Entry) {
        let res: Result<(), ()>;
        {
            let s = self.phy_ch_a.text();
            let mut d = self.dati.borrow_mut();
            res = d.mem.page_pa_from_string(bubble::Channel::ChannelA, &s);
        }
        if res.is_ok() {
            self.update_curr_page();
        } else {
            self.alert_no_parse();
        }
    }
    #[template_callback]
    fn handle_phy_ch_b_activate(&self, _: &Entry) {
        let res: Result<(), ()>;
        {
            let s = self.phy_ch_b.text();
            let mut d = self.dati.borrow_mut();
            res = d.mem.page_pa_from_string(bubble::Channel::ChannelB, &s);
        }
        if res.is_ok() {
            self.update_curr_page();
        } else {
            self.alert_no_parse();
        }
    }

    fn ask_yes_no<P: FnOnce(bool) + 'static>(
        &self,
        msg: &str,
        detail: &str,
        def: bool,
        have_cancel: bool,
        callback: P,
    ) {
        let alert_bld = AlertDialog::builder()
            .default_button(if def { 0 } else { 1 })
            .modal(true)
            .message(msg)
            .detail(detail);
        let alert_bld2: AlertDialogBuilder = if have_cancel {
            alert_bld.buttons(["Yes", "No", "Cancel"]).cancel_button(2)
        } else {
            alert_bld.buttons(["Yes", "No"])
        };
        let alert = alert_bld2.build();
        let c = self.obj();
        let app_window = Some(c.upcast_ref::<gtk::Window>());
        alert.choose(
            app_window,
            Cancellable::NONE,
            clone!(
                #[weak(rename_to = win)]
                self,
                move |res| win.ask_yes_no_2(res, callback)
            ),
        );
    }
    fn ask_yes_no_2<P: FnOnce(bool) + 'static>(&self, res: Result<i32, Error>, callback: P) {
        if let Ok(choice) = res {
            if choice != 2 {
                callback(choice == 0);
            }
        }
    }
    fn bubble_file_dialog(&self) -> FileDialog {
        let ls = ListStore::builder()
            .item_type(FileFilter::static_type())
            .build();
        let ff1 = FileFilter::new();
        ff1.add_suffix("imbm");
        ls.append(&ff1);
        FileDialog::builder().modal(true).filters(&ls).build()
    }
    fn check_saving_needed<P: FnOnce() + 'static>(&self, force: bool, callback: P) {
        let d = self.dati.borrow().mem.is_dirty();
        if d {
            if force {
                self.check_2(callback);
            } else {
                self.ask_yes_no(
                    "File is not saved",
                    "Save it?",
                    true,
                    true,
                    clone!(
                        #[weak(rename_to = win)]
                        self,
                        move |res| if res {
                            win.check_2(callback);
                        } else {
                            callback();
                        }
                    ),
                );
            }
        } else {
            callback();
        }
    }
    fn check_2<P: FnOnce() + 'static>(&self, callback: P) {
        {
            let d = self.dati.borrow();
            if d.path.as_os_str().is_empty() {
                let fd = self.bubble_file_dialog();
                fd.set_title("Save as..");
                let c = self.obj();
                let app_window = Some(c.upcast_ref::<gtk::Window>());
                fd.save(
                    app_window,
                    Cancellable::NONE,
                    clone!(
                        #[weak(rename_to = win)]
                        self,
                        move |res| win.check_3(res, callback)
                    ),
                );
                return;
            }
        }
        if self.do_file_save() {
            callback();
        }
    }
    fn check_3<P: FnOnce() + 'static>(&self, res: Result<File, Error>, callback: P) {
        if let Ok(file) = res {
            if let Some(path) = file.path() {
                {
                    let mut d = self.dati.borrow_mut();
                    d.path = path;
                }
                if self.do_file_save() {
                    callback();
                } else {
                    let mut d = self.dati.borrow_mut();
                    d.path = PathBuf::new();
                }
            }
        }
    }
    fn do_file_save(&self) -> bool {
        let mut out: bool = false;
        {
            let mut d = self.dati.borrow_mut();
            let f = fs::File::create(d.path.clone());
            if let Ok(mut out_file) = f {
                let res = d.mem.save(&mut out_file);
                if res.is_ok() {
                    out = true;
                } else if let Err(err) = res {
                    self.alert_msg(&format!("Output error: {}", err));
                }
            } else if let Err(err) = f {
                self.alert_msg(&format!("Can't create file: {}", err));
            }
        }
        if out {
            self.set_title();
        }
        out
    }
    pub fn file_new(&self) {
        self.check_saving_needed(
            false,
            clone!(
                #[weak(rename_to = win)]
                self,
                move || win.file_new_2()
            ),
        );
    }
    fn file_new_2(&self) {
        {
            let mut d = self.dati.borrow_mut();
            d.mem.clear();
            d.path = PathBuf::new();
        }
        self.update_curr_page();
    }
    pub fn file_load(&self) {
        self.check_saving_needed(
            false,
            clone!(
                #[weak(rename_to = win)]
                self,
                move || win.file_load_2()
            ),
        );
    }
    fn file_load_2(&self) {
        let fd = self.bubble_file_dialog();
        fd.set_title("File to load");
        let c = self.obj();
        let app_window = Some(c.upcast_ref::<gtk::Window>());
        fd.open(
            app_window,
            Cancellable::NONE,
            clone!(
                #[weak(rename_to = win)]
                self,
                move |res| win.file_load_3(res)
            ),
        );
    }
    fn file_load_3(&self, res: Result<File, Error>) {
        if let Ok(file) = res {
            if let Some(path) = file.path() {
                let path_clone = path.clone();
                let f = fs::File::open(path);
                if let Ok(mut in_file) = f {
                    let res: io::Result<()>;
                    {
                        let mut d = self.dati.borrow_mut();
                        res = d.mem.load(&mut in_file);
                    }
                    if res.is_ok() {
                        let mut d = self.dati.borrow_mut();
                        d.path = path_clone;
                    }
                    if res.is_ok() {
                        self.update_curr_page();
                    }
                    if let Err(err) = res {
                        self.alert_msg(&format!("Can't load file: {}", err));
                    }
                } else if let Err(err) = f {
                    self.alert_msg(&format!("Can't open file: {}", err));
                }
            }
        }
    }
    pub fn file_save(&self) {
        self.check_saving_needed(true, || {});
    }
    fn check_img_change<P: FnOnce() + 'static>(&self, saving: bool, callback: P) {
        let bl_valid = self.dati.borrow().mem.is_bl_valid();
        if bl_valid {
            if saving {
                self.check_saving_needed(false, callback);
            } else {
                callback();
            }
        } else {
            self.alert_msg("Bootloop is invalid");
        }
    }
    pub fn img_clear(&self) {
        self.check_img_change(
            true,
            clone!(
                #[weak(rename_to = win)]
                self,
                move || win.img_clear_2()
            ),
        );
    }
    pub fn img_clear_2(&self) {
        {
            let mut d = self.dati.borrow_mut();
            d.mem.data_clear();
        }
        self.update_curr_page();
    }
    fn image_file_dialog(&self) -> FileDialog {
        let ls = ListStore::builder()
            .item_type(FileFilter::static_type())
            .build();
        let ff1 = FileFilter::new();
        ff1.add_suffix("bin");
        ls.append(&ff1);
        let ff2 = FileFilter::new();
        ff2.add_suffix("img");
        ls.append(&ff2);
        FileDialog::builder().modal(true).filters(&ls).build()
    }
    pub fn img_load(&self) {
        self.check_img_change(
            true,
            clone!(
                #[weak(rename_to = win)]
                self,
                move || win.img_load_2()
            ),
        );
    }
    pub fn img_load_2(&self) {
        let fd = self.image_file_dialog();
        fd.set_title("Image to load");
        let c = self.obj();
        let app_window = Some(c.upcast_ref::<gtk::Window>());
        fd.open(
            app_window,
            Cancellable::NONE,
            clone!(
                #[weak(rename_to = win)]
                self,
                move |res| win.img_load_3(res)
            ),
        );
    }
    pub fn img_load_3(&self, res: Result<File, Error>) {
        if let Ok(file) = res {
            if let Some(path) = file.path() {
                let f = fs::File::open(path);
                if let Ok(mut in_file) = f {
                    let res: io::Result<()>;
                    {
                        let mut d = self.dati.borrow_mut();
                        res = d.mem.data_load(&mut in_file);
                    }
                    if res.is_ok() {
                        self.update_curr_page();
                    }
                    if let Err(err) = res {
                        self.alert_msg(&format!("Can't load file: {}", err));
                    }
                } else if let Err(err) = f {
                    self.alert_msg(&format!("Can't open file: {}", err));
                }
            }
        }
    }
    pub fn img_save(&self) {
        self.check_img_change(
            false,
            clone!(
                #[weak(rename_to = win)]
                self,
                move || win.img_save_2()
            ),
        );
    }
    pub fn img_save_2(&self) {
        let fd = self.image_file_dialog();
        fd.set_title("Save image as..");
        let c = self.obj();
        let app_window = Some(c.upcast_ref::<gtk::Window>());
        fd.save(
            app_window,
            Cancellable::NONE,
            clone!(
                #[weak(rename_to = win)]
                self,
                move |res| win.img_save_3(res)
            ),
        );
    }
    pub fn img_save_3(&self, res: Result<File, Error>) {
        if let Ok(file) = res {
            if let Some(path) = file.path() {
                let f = fs::File::create(path);
                if let Ok(mut out_file) = f {
                    let res: io::Result<(usize, usize, usize)>;
                    {
                        let mut d = self.dati.borrow_mut();
                        res = d.mem.data_save(&mut out_file);
                    }
                    if let Ok(cnt) = res {
                        self.disp_counters(cnt);
                        self.update_curr_page();
                    }
                    if let Err(err) = res {
                        self.alert_msg(&format!("Output error: {}", err));
                    }
                } else if let Err(err) = f {
                    self.alert_msg(&format!("Can't create file: {}", err));
                }
            }
        }
    }
    fn disp_counters(&self, cnt: (usize, usize, usize)) {
        let s = format!(
            "Ok: {}\nCorrected: {}\nUncorrectable: {}",
            cnt.0, cnt.1, cnt.2
        );
        self.alert_msg(&s);
    }
    pub fn img_fix(&self) {
        self.check_img_change(
            false,
            clone!(
                #[weak(rename_to = win)]
                self,
                move || win.img_fix_2()
            ),
        );
    }
    pub fn img_fix_2(&self) {
        let res: Option<(usize, usize, usize)>;
        {
            let mut d = self.dati.borrow_mut();
            res = d.mem.data_fix(true);
        }
        if let Some(cnt) = res {
            self.disp_counters(cnt);
            self.update_curr_page();
        } else {
            self.alert_msg("Fix failed");
        }
    }
    pub fn img_stat(&self) {
        let res: Option<(usize, usize, usize)>;
        {
            let mut d = self.dati.borrow_mut();
            res = d.mem.data_fix(false);
        }
        if let Some(cnt) = res {
            self.disp_counters(cnt);
        }
    }
    pub fn about(&self) {
        let c = self.obj();
        let app_window = c.upcast_ref::<gtk::Window>();
        let abt = AboutDialog::builder()
            .transient_for(app_window)
            .modal(true)
            .comments(env!("CARGO_PKG_DESCRIPTION"))
            .copyright("© 2026 F.Ulivi")
            .authors([env!("CARGO_PKG_AUTHORS")])
            .website(env!("CARGO_PKG_HOMEPAGE"))
            .license_type(gtk::License::Gpl20)
            .version(env!("CARGO_PKG_VERSION"))
            .build();
        abt.present();
    }
}
