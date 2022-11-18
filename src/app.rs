use std::{
	fs::read_dir,
	io::Cursor,
	path::{Path, PathBuf}
};

use dirs::home_dir;
use egui::{CentralPanel, Context, RichText};
use human_bytes::human_bytes;
use mslnk::ShellLink;
use poll_promise::Promise;
use rfd::FileDialog;
use zip_extract::extract;

pub struct App {
	game_folder: PathBuf,
	download_size: f64,
	download_promise: Option<Promise<Vec<u8>>>,
	installation_done: bool
}

impl App {
	/// Called once before the first frame.
	pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
		App {
			game_folder: PathBuf::new(),
			download_size: if let Ok(data) = reqwest::blocking::Client::new().head("https://github.com/atampy25/simple-mod-framework/releases/latest/download/Release.zip").send() {
				data.headers().get("Content-Length").unwrap().to_str().unwrap().parse().unwrap()
			} else {
				-1.0
			},
			download_promise: None,
			installation_done: false
		}
	}
}

impl eframe::App for App {
	fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
		ctx.set_pixels_per_point(3.0);

		CentralPanel::default().show(ctx, |ui| {
			ui.label(RichText::from("Welcome to the Simple Mod Framework!").strong());

			ui.label("Let's find your game folder.");

			ui.label(
				RichText::from(
					"The framework will only work if you're using a copy of the game bought from \
					 Steam, Epic Games or Microsoft."
				)
				.size(8.0)
			);

			let mut set_game_path = false;

			ui.horizontal_wrapped(|ui| {
				let mut path_not_valid = false;
				let mut game_folder_set_automatically = false;

				for path in [
					r#"C:\Program Files\Epic Games\HITMAN3"#,
					r#"C:\Program Files (x86)\Steam\steamapps\common\HITMAN 3"#
				] {
					let path = PathBuf::from(path);

					if Path::new(&path.join("Retail")).is_dir()
						&& (Path::new(&path.join("Runtime")).is_dir()
							|| Path::new(&path.join("Retail").join("Runtime")).is_dir())
					{
						self.game_folder = path;
						game_folder_set_automatically = true;
					}
				}

				if self.game_folder.to_str().unwrap() != "" {
					ui.label(RichText::from(self.game_folder.to_str().unwrap()).size(7.0));
					if Path::new(&self.game_folder.join("Retail")).is_dir()
						&& (Path::new(&self.game_folder.join("Runtime")).is_dir()
							|| Path::new(&self.game_folder.join("Retail").join("Runtime")).is_dir())
					{
						if game_folder_set_automatically {
							ui.label(
								RichText::from("✅ Game folder found automatically").size(7.0)
							);
						} else {
							ui.label(RichText::from("✅ Game folder selected").size(7.0));
						}
						set_game_path = true;
						return;
					} else {
						path_not_valid = true;
					}
				}

				if path_not_valid {
					ui.label(RichText::from("❌ Not a game folder").size(7.0));
				}

				if ui
					.button(RichText::from("Select your game folder").size(7.0))
					.clicked()
				{
					if let Some(mut folder) = FileDialog::new()
						.set_title(
							"Select your game folder; it should contain a folder called Retail"
						)
						.pick_folder()
					{
						if let Some(x) = read_dir(&folder).unwrap().next() {
							if x.as_ref().unwrap().path().join("Retail").is_dir() {
								folder = x.unwrap().path();
							}
						}

						self.game_folder = folder;
					}
				}
			});

			if set_game_path {
				ui.label("Ready to install the framework?");

				if self.download_size != -1.0 {
					ui.label(
						RichText::from(format!(
							"This will download {} of data.",
							human_bytes(self.download_size)
						))
						.size(8.0)
					);

					if self.download_promise.is_none()
						&& ui
							.button(RichText::from("Install the framework").size(7.0))
							.clicked()
					{
						self.download_promise.get_or_insert_with(|| {
							let ctx = ctx.clone();
							let (sender, promise) = Promise::new();

							let request =
								ehttp::Request::get("https://github.com/atampy25/simple-mod-framework/releases/latest/download/Release.zip");

							ehttp::fetch(request, move |response| {
								let data = response.unwrap().bytes;
								sender.send(data);
								ctx.request_repaint();
							});
							promise
						});
					}

					ui.add_space(5.0);

					if let Some(promise) = &self.download_promise {
						match promise.ready() {
							None => {
								ui.spinner();
							}
							Some(data) => {
								if !self.installation_done {
									extract(
										Cursor::new(data),
										&self.game_folder.join("Simple Mod Framework"),
										false
									)
									.unwrap();

									ShellLink::new(
										self.game_folder
											.join("Simple Mod Framework")
											.join("Mod Manager")
											.join("Mod Manager.exe")
											.to_str()
											.unwrap()
									)
									.unwrap()
									.create_lnk(
										home_dir()
											.unwrap()
											.join("AppData")
											.join("Roaming")
											.join("Microsoft")
											.join("Windows")
											.join("Start Menu")
											.join("Programs")
											.join("Simple Mod Framework.lnk")
									)
									.unwrap();

									self.installation_done = true;
								}

								ui.add_space(4.0);

								ui.label("Installation done!");

								ui.label(
									RichText::from(
										"You can close this window; a shortcut has been added to \
										 the Start menu."
									)
									.size(8.0)
								);
							}
						}
					}
				} else {
					ui.label(
						RichText::from(
							"It seems you don't have access to the internet. Try again when \
							 connected to an unrestricted network."
						)
						.size(8.0)
					);
				}
			}
		});
	}
}
