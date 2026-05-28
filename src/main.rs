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

mod bubble;
mod window;

use gtk::gdk::Display;
use gtk::glib::subclass::types::ObjectSubclassIsExt;
use gtk::prelude::*;
use gtk::{Application, CssProvider, gio, glib};
use window::Window;

const APP_ID: &str = "org.bubbledit-gtk";

fn main() -> glib::ExitCode {
    gio::resources_register_include!("composite_templates_1.gresource")
        .expect("Failed to register resources.");

    let app = Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();

    // Connect to signals
    app.connect_startup(|app| {
        setup_shortcuts(app);
        load_css()
    });
    app.connect_activate(build_ui);
    app.connect_open(file_open);

    app.run()
}
fn setup_shortcuts(app: &Application) {
    app.set_accels_for_action("window.close", &["<Ctrl>Q"]);
    app.set_accels_for_action("my.new", &["<Ctrl>N"]);
    app.set_accels_for_action("my.load", &["<Ctrl>O"]);
    app.set_accels_for_action("my.save", &["<Ctrl>S"]);
    app.set_accels_for_action("my.img_clear", &["<Shift><Ctrl>N"]);
    app.set_accels_for_action("my.img_load", &["<Shift><Ctrl>O"]);
    app.set_accels_for_action("my.img_save", &["<Shift><Ctrl>S"]);
    app.set_accels_for_action("my.img_fix", &["<Shift><Ctrl>F"]);
}
fn load_css() {
    // Load the CSS file and add it to the provider
    let provider = CssProvider::new();
    provider.load_from_resource("/bubbledit-gtk/style.css");

    // Add the provider to the default screen
    gtk::style_context_add_provider_for_display(
        &Display::default().expect("Could not connect to a display."),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
fn build_ui(app: &Application) {
    let window = Window::new(app);
    window.present();
}
fn file_open(app: &Application, f: &[gio::File], _: &str) {
    let window = Window::new(app);
    if !f.is_empty() {
        window.imp().open_file(f[0].clone())
    }
    window.present();
}
