#![warn(clippy::all, rust_2018_idioms)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use egui::Vec2;

fn main() {
	tracing_subscriber::fmt::init();

	let native_options = eframe::NativeOptions {
		initial_window_size: Some(Vec2 {
			x: 1200.0,
			y: 700.0
		}),
		icon_data: Some(load_icon(include_bytes!("icon.png"))),
		..Default::default()
	};
	eframe::run_native(
		"Simple Mod Framework Installer",
		native_options,
		Box::new(|_| Box::new(simple_mod_framework_installer::App::new()))
	);
}

fn load_icon(data: &[u8]) -> eframe::IconData {
	let (icon_rgba, icon_width, icon_height) = {
		let image = image::load_from_memory(data).unwrap().into_rgba8();
		let (width, height) = image.dimensions();
		let rgba = image.into_raw();
		(rgba, width, height)
	};

	eframe::IconData {
		rgba: icon_rgba,
		width: icon_width,
		height: icon_height
	}
}
