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
use gtk::gio::prelude::FileExt;
use gtk::gio::{Cancellable, File, ListStore};
use gtk::glib::{self, Error};
use gtk::prelude::{BoxExt, ButtonExt, GtkWindowExt, StaticType, WidgetExt};
use gtk::{
    AlertDialog, Align, Box as GtkBox, Button, FileDialog, FileFilter, HeaderBar, Label,
    Orientation, PolicyType, ScrolledWindow, Window,
};
use std::fs;
use std::io::{self, Cursor};
use std::path::{Path, PathBuf};
use std::rc::Rc;

struct OutputFile {
    name: String,
    data: Vec<u8>,
}

pub fn open(parent: &Window) {
    let fd = img_file_dialog();
    fd.set_title("Image to convert");
    fd.open(
        Some(parent),
        Cancellable::NONE,
        clone!(
            #[strong]
            parent,
            move |res| open_2(&parent, res)
        ),
    );
}

fn open_2(parent: &Window, res: Result<File, Error>) {
    if let Ok(file) = res {
        if let Some(path) = file.path() {
            match validate_image(&path) {
                Ok(parts) => show(parent, path, parts),
                Err(err) => alert(parent, &err.to_string()),
            }
        }
    }
}

fn img_file_dialog() -> FileDialog {
    let ls = ListStore::builder()
        .item_type(FileFilter::static_type())
        .build();
    let ff1 = FileFilter::new();
    ff1.add_suffix("img");
    ls.append(&ff1);
    FileDialog::builder().modal(true).filters(&ls).build()
}

fn validate_image(path: &Path) -> io::Result<usize> {
    let len = fs::metadata(path)?.len();
    let img_size = bubble::IMG_SIZE as u64;
    if len == 0 {
        Err(io::Error::other("Input file is empty"))
    } else if len % img_size != 0 {
        Err(io::Error::other(
            "Input file size is not a multiple of 128 KiB",
        ))
    } else {
        Ok((len / img_size) as usize)
    }
}

fn show(parent: &Window, input_path: PathBuf, parts: usize) {
    let win_bld = Window::builder()
        .transient_for(parent)
        .modal(true)
        .title("IMG file converter")
        .default_width(480);
    let win = if let Some(app) = parent.application() {
        win_bld.application(&app).build()
    } else {
        win_bld.build()
    };
    let header = HeaderBar::builder().show_title_buttons(true).build();
    win.set_titlebar(Some(&header));
    win.add_css_class("dialog");

    let box_main = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();

    let title = Label::builder()
        .label("Convert file")
        .halign(Align::Center)
        .build();
    title.add_css_class("title-1");
    box_main.append(&title);

    let filename = display_name(&input_path);
    let input_label = Label::builder().halign(Align::Center).build();
    input_label.set_markup(&format!("<b>{}</b>", glib::markup_escape_text(&filename)));
    input_label.add_css_class("heading");
    box_main.append(&input_label);

    let arrow = Label::builder().label("↓").halign(Align::Center).build();
    arrow.add_css_class("dim-label");
    box_main.append(&arrow);

    let list_box = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(6)
        .build();
    let names = output_names(&input_path, parts);
    for name in &names {
        let label = Label::builder().label(name).halign(Align::Center).build();
        list_box.append(&label);
    }
    let scroll = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .min_content_height(80)
        .max_content_height(240)
        .child(&list_box)
        .build();
    box_main.append(&scroll);

    let button = Button::builder()
        .label("Convert")
        .halign(Align::Center)
        .build();
    button.add_css_class("suggested-action");
    button.connect_clicked(clone!(
        #[weak]
        win,
        #[strong]
        input_path,
        move |_| convert_clicked(&win, &input_path)
    ));
    box_main.append(&button);

    win.set_child(Some(&box_main));
    win.present();
}

fn convert_clicked(parent: &Window, input_path: &Path) {
    match convert(input_path) {
        Ok(files) => select_output_folder(parent, files),
        Err(err) => alert(parent, &format!("Can't convert file: {}", err)),
    }
}

fn convert(input_path: &Path) -> io::Result<Vec<OutputFile>> {
    let input = fs::read(input_path)?;
    if input.is_empty() || input.len() % bubble::IMG_SIZE != 0 {
        return Err(io::Error::other(
            "Input file size is not a multiple of 128 KiB",
        ));
    }
    let parts = input.len() / bubble::IMG_SIZE;
    let names = output_names(input_path, parts);
    let mut output = Vec::<OutputFile>::new();
    for (name, image) in names.into_iter().zip(split_image_chunks(&input)) {
        let mut input_cursor = Cursor::new(&image);
        let mut mem = bubble::Memory::new();
        mem.clear();
        mem.data_load(&mut input_cursor)?;
        let mut data = Vec::<u8>::new();
        mem.save(&mut data)?;
        output.push(OutputFile { name, data });
    }
    Ok(output)
}

fn split_image_chunks(input: &[u8]) -> Vec<Vec<u8>> {
    input
        .chunks_exact(bubble::IMG_SIZE)
        .map(|chunk| chunk.to_vec())
        .collect()
}

fn select_output_folder(parent: &Window, files: Vec<OutputFile>) {
    let files = Rc::new(files);
    let fd = FileDialog::builder()
        .modal(true)
        .title("Select output folder")
        .build();
    fd.select_folder(
        Some(parent),
        Cancellable::NONE,
        clone!(
            #[weak]
            parent,
            #[strong]
            files,
            move |res| save_files(&parent, res, files.clone())
        ),
    );
}

fn save_files(parent: &Window, res: Result<File, Error>, files: Rc<Vec<OutputFile>>) {
    if let Ok(folder) = res {
        if let Some(path) = folder.path() {
            if let Err(err) = write_files(&path, &files) {
                alert(parent, &format!("Can't save files: {}", err));
            } else {
                saved_dialog(parent);
            }
        }
    }
}

fn write_files(folder: &Path, files: &[OutputFile]) -> io::Result<()> {
    for file in files {
        fs::write(folder.join(&file.name), &file.data)?;
    }
    Ok(())
}

fn output_names(input_path: &Path, parts: usize) -> Vec<String> {
    let stem = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    (0..parts)
        .map(|idx| format!("{}_{}.imbm", stem, idx))
        .collect()
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Selected image")
        .to_string()
}

fn alert(parent: &Window, msg: &str) {
    let alert = AlertDialog::builder().modal(true).message(msg).build();
    alert.show(Some(parent));
}

fn saved_dialog(parent: &Window) {
    let win_bld = Window::builder()
        .transient_for(parent)
        .modal(true)
        .title("Success")
        .default_width(320);
    let win = if let Some(app) = parent.application() {
        win_bld.application(&app).build()
    } else {
        win_bld.build()
    };
    let header = HeaderBar::builder().show_title_buttons(false).build();
    win.set_titlebar(Some(&header));
    win.add_css_class("dialog");

    let box_main = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(18)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();

    let label = Label::builder()
        .label("All files saved")
        .halign(Align::Center)
        .build();
    label.add_css_class("title-3");
    box_main.append(&label);

    let button = Button::builder()
        .label("OK")
        .halign(Align::Center)
        .can_focus(false)
        .focus_on_click(false)
        .build();
    button.add_css_class("suggested-action");
    button.connect_clicked(clone!(
        #[weak]
        win,
        #[weak]
        parent,
        move |_| {
            win.close();
            parent.close();
        }
    ));
    box_main.append(&button);

    win.set_child(Some(&box_main));
    win.present();
}
