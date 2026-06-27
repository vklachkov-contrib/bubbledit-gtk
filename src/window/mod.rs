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

mod converter;
mod imp;

use glib::Object;
use glib::clone;
use gtk::{
    Application,
    gio::{self, ActionEntryBuilder, SimpleActionGroup, prelude::ActionMapExtManual},
    glib::{self, subclass::types::ObjectSubclassIsExt},
    prelude::WidgetExt,
};

glib::wrapper! {
    pub struct Window(ObjectSubclass<imp::Window>)
        @extends gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
                    gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl Window {
    pub fn new(app: &Application) -> Self {
        // Create new window
        Object::builder()
            .property("application", app)
            .property("default-width", 1500)
            .property("resizable", false)
            .build()
    }
    fn setup_actions(&self) {
        let actions = SimpleActionGroup::new();
        let win = self.imp();
        let action_new = ActionEntryBuilder::<SimpleActionGroup>::new("new")
            .activate(clone!(
                #[weak]
                win,
                move |_, _, _| {
                    win.file_new();
                }
            ))
            .build();
        let action_load = ActionEntryBuilder::<SimpleActionGroup>::new("load")
            .activate(clone!(
                #[weak]
                win,
                move |_, _, _| {
                    win.file_load();
                }
            ))
            .build();
        let action_save = ActionEntryBuilder::<SimpleActionGroup>::new("save")
            .activate(clone!(
                #[weak]
                win,
                move |_, _, _| {
                    win.file_save();
                }
            ))
            .build();
        let action_img_clear = ActionEntryBuilder::<SimpleActionGroup>::new("img_clear")
            .activate(clone!(
                #[weak]
                win,
                move |_, _, _| {
                    win.img_clear();
                }
            ))
            .build();
        let action_img_load = ActionEntryBuilder::<SimpleActionGroup>::new("img_load")
            .activate(clone!(
                #[weak]
                win,
                move |_, _, _| {
                    win.img_load();
                }
            ))
            .build();
        let action_img_save = ActionEntryBuilder::<SimpleActionGroup>::new("img_save")
            .activate(clone!(
                #[weak]
                win,
                move |_, _, _| {
                    win.img_save();
                }
            ))
            .build();
        let action_img_fix = ActionEntryBuilder::<SimpleActionGroup>::new("img_fix")
            .activate(clone!(
                #[weak]
                win,
                move |_, _, _| {
                    win.img_fix();
                }
            ))
            .build();
        let action_img_stat = ActionEntryBuilder::<SimpleActionGroup>::new("img_stat")
            .activate(clone!(
                #[weak]
                win,
                move |_, _, _| {
                    win.img_stat();
                }
            ))
            .build();
        let action_convert_img = ActionEntryBuilder::<SimpleActionGroup>::new("convert_img")
            .activate(clone!(
                #[weak]
                win,
                move |_, _, _| {
                    win.convert_img();
                }
            ))
            .build();
        let action_about = ActionEntryBuilder::<SimpleActionGroup>::new("about")
            .activate(clone!(
                #[weak]
                win,
                move |_, _, _| {
                    win.about();
                }
            ))
            .build();
        actions.add_action_entries([
            action_new,
            action_load,
            action_save,
            action_img_clear,
            action_img_load,
            action_img_save,
            action_img_fix,
            action_img_stat,
            action_convert_img,
            action_about,
        ]);
        self.insert_action_group("my", Some(&actions));
    }
}
