use std::{
	collections::HashMap,
	fs,
	io::Cursor,
	os::windows::process::CommandExt,
	path::{Path, PathBuf},
	process::Command
};

use anyhow::Context;
use base64::{engine::general_purpose, Engine};
use dirs::home_dir;
use egui::{CentralPanel, Color32, ComboBox, Context as EguiContext, RichText};
use human_bytes::human_bytes;
use ini::Ini;
use mslnk::ShellLink;
use poll_promise::Promise;
use registry::{Data, Hive, Security};
use serde::Deserialize;
use serde_json::Value;
use zip_extract::extract;

// This is just so that rustfmt doesn't completely stop formatting the codebase
// (it has an issue with print width that causes it to not format the whole function)
static FRAMEWORK_DOWNLOAD_URL: &str =
	"https://github.com/atampy25/simple-mod-framework/releases/latest/download/Release.zip";

pub struct App {
	download_size: Option<f64>,
	download_promise: Option<Promise<Result<Vec<u8>, String>>>,
	installation_done: bool,
	error: Option<String>,
	performed_automatic_check: bool,
	valid_game_folders: Vec<(PathBuf, Option<String>)>,
	selected_game_folder: Option<usize>
}

#[derive(Deserialize)]
struct SteamLibraryFolder {
	path: String,
	apps: HashMap<String, String>
}

#[derive(Deserialize)]
struct SteamUser {
	#[serde(rename = "PersonaName")]
	persona_name: String,

	#[serde(rename = "MostRecent")]
	most_recent: bool
}

impl App {
	/// Called once before the first frame.
	pub fn new() -> Self {
		let download_size = (|| -> anyhow::Result<f64> {
			Ok(reqwest::blocking::Client::new()
				.head(FRAMEWORK_DOWNLOAD_URL)
				.send()?
				.headers()
				.get("Content-Length")
				.context("Content-Length")?
				.to_str()?
				.parse()?)
		})();

		App {
			download_size: download_size.ok(),
			download_promise: None,
			installation_done: false,
			error: None,
			performed_automatic_check: false,
			valid_game_folders: vec![],
			selected_game_folder: None
		}
	}
}

impl Default for App {
	fn default() -> Self {
		Self::new()
	}
}

impl eframe::App for App {
	fn update(&mut self, ctx: &EguiContext, _frame: &mut eframe::Frame) {
		ctx.set_pixels_per_point(3.0);

		if let Some(error) = &self.error {
			CentralPanel::default().show(ctx, |ui| {
				ui.label(RichText::from("Welcome to the Simple Mod Framework!").strong());

				ui.label(
					"It seems a critical error has occurred. Try restarting the installation \
					 process, or give Atampy26 the following error message on Hitman Forum (note \
					 that this does not say Nexus Mods)."
				);

				ui.label(
					RichText::from(error)
						.color(Color32::from_rgb(200, 50, 50))
						.size(7.0)
				);
			});
		} else {
			CentralPanel::default().show(ctx, |ui| {
				ui.label(RichText::from("Welcome to the Simple Mod Framework!").strong());

				ui.label("Let's find your game folder.");

				ui.label(
					RichText::from(
						"The framework will only work if you're using a copy of the game bought \
						 from Steam, Epic Games or Microsoft."
					)
					.size(8.0)
				);

				ui.label(
					RichText::from(
						"If you're using a Microsoft version of the game (Xbox/Game Pass), you \
						 may need to enable the \"Advanced management features\" option to be \
						 able to install the framework. When that's done, you should be able to \
						 select the game folder here."
					)
					.size(8.0)
				);

				ui.add_space(2.5);

				let x = (|| -> anyhow::Result<()> {
					if !self.performed_automatic_check {
						let mut check_paths = vec![];

						let legendary_installed_path =
							Path::new(&std::env::var("USERPROFILE").context("%USERPROFILE%")?)
								.join(".config")
								.join("legendary")
								.join("installed.json");

						// Check for a Legendary install
						if legendary_installed_path.exists() {
							let legendary_installed_data: Value = serde_json::from_slice(
								&fs::read(legendary_installed_path)
									.context("Reading legendary installed")?
							)?;

							if let Some(data) = legendary_installed_data.get("Eider") {
								check_paths.push((
									PathBuf::from(
										data.get("install_path")
											.context("install_path")?
											.as_str()
											.context("as_str")?
									),
									Some(
										serde_json::from_slice::<Value>(&fs::read(
											Path::new(&std::env::var("USERPROFILE")?)
												.join(".config")
												.join("legendary")
												.join("user.json")
										)?)?
										.get("displayName")
										.context("displayName")?
										.as_str()
										.context("as_str")?
										.to_owned()
									)
								));
							}
						}

						// Check for EOS manifests
						if let Ok(hive) =
							Hive::CurrentUser.open(r#"Software\Epic Games\EOS"#, Security::Read)
						{
							match hive.value("ModSdkMetadataDir") {
								Ok(Data::String(d)) => {
									for entry in fs::read_dir(d.to_string_lossy())?
										.filter_map(|x| x.ok())
										.filter(|x| {
											x.file_type().ok().map(|x| x.is_file()).unwrap_or(false)
										}) {
										let manifest_data: Value = serde_json::from_slice(
											&fs::read(entry.path()).with_context(|| {
												format!(
													"{}{}",
													"EOS manifest",
													entry.path().display()
												)
											})?
										)?;

										if manifest_data
											.get("AppName")
											.context("AppName")?
											.as_str()
											.context("as_str")? == "Eider"
										{
											let mut username = None;

											if Path::new(&std::env::var("LOCALAPPDATA")?)
												.join("EpicGamesLauncher")
												.join("Saved")
												.join("Config")
												.join("Windows")
												.join("GameUserSettings.ini")
												.exists()
											{
												if let Some(x) = Ini::load_from_file(
													Path::new(&std::env::var("LOCALAPPDATA")?)
														.join("EpicGamesLauncher")
														.join("Saved")
														.join("Config")
														.join("Windows")
														.join("GameUserSettings.ini")
												)?
												.section(Some("Offline"))
												.and_then(|x| x.get("Data"))
												{
													username = Some(
														serde_json::from_slice::<Value>(
															&general_purpose::STANDARD.decode(x)?
														)?
														.get(0)
														.context("get 0")?
														.get("DisplayName")
														.context("DisplayName")?
														.as_str()
														.context("as_str")?
														.to_owned()
													);
												};
											}

											check_paths.push((
												PathBuf::from(
													manifest_data
														.get("InstallLocation")
														.context("InstallLocation")?
														.as_str()
														.context("as_str")?
												),
												username
											));
										}
									}
								}

								Ok(_) => Err(anyhow::anyhow!("Registry key ModSdkMetadataDir \
								                              was not string"
									.to_owned()))?,

								Err(_) => {}
							}
						}

						// Check for a Steam install
						if let Ok(hive) =
							Hive::CurrentUser.open(r#"Software\Valve\Steam"#, Security::Read)
						{
							match hive.value("SteamPath") {
								Ok(Data::String(d)) => {
									if let Ok(s) = fs::read_to_string(
										if Path::new(&d.to_string_lossy())
											.join("config")
											.join("libraryfolders.vdf")
											.exists()
										{
											Path::new(&d.to_string_lossy())
												.join("config")
												.join("libraryfolders.vdf")
										} else {
											Path::new(&d.to_string_lossy())
												.join("steamapps")
												.join("libraryfolders.vdf")
										}
									) {
										let folders: HashMap<String, SteamLibraryFolder> =
											keyvalues_serde::from_str(&s).context("VDF parse")?;

										for folder in folders.values() {
											if folder.apps.contains_key("1659040")
												|| folder.apps.contains_key("1847520")
											{
												let users: HashMap<String, SteamUser> =
													keyvalues_serde::from_str(
														&fs::read_to_string(
															Path::new(&d.to_string_lossy())
																.join("config")
																.join("loginusers.vdf")
														)?
													)?;

												check_paths.push((
													Path::new(&folder.path)
														.join("steamapps")
														.join("common")
														.join("HITMAN 3"),
													Some(
														users
															.values()
															.find(|x| x.most_recent)
															.context("Most recent user")?
															.persona_name
															.to_owned()
													)
												));
											}
										}
									};
								}

								Ok(_) => {
									self.error =
										Some("Registry key SteamPath was not string".to_owned());
								}

								Err(_) => {}
							}
						}

						// Check for a Microsoft install
						if let Ok(proc_out) = Command::new("powershell")
							.args([
								"-Command",
								"Get-AppxPackage -Name IOInteractiveAS.PC-HITMAN3-BaseGame"
							])
							.creation_flags(0x08000000) // CREATE_NO_WINDOW
							.output()
						{
							if let Some(line) = String::from_utf8_lossy(&proc_out.stdout)
								.lines()
								.find(|x| x.starts_with("InstallLocation"))
							{
								let mut username = None;

								if let Ok(hive) = Hive::CurrentUser
									.open(r#"Software\Microsoft\XboxLive"#, Security::Read)
								{
									if let Ok(Data::String(d)) = hive.value("ModernGamertag") {
										username = Some(d.to_string_lossy());
									}
								}

								check_paths.push((
									PathBuf::from(
										line.split(':')
											.skip(1)
											.collect::<Vec<_>>()
											.join(":")
											.trim()
									),
									username
								));
							}
						}

						for (path, username) in check_paths {
							// Game folder has Retail
							let subfolder_retail = path.join("Retail").is_dir();

							// Game folder has Runtime or Retail/Runtime
							let subfolder_runtime = path.join("Runtime").is_dir()
								|| path.join("Retail").join("Runtime").is_dir();

							// User is not trying to install the framework on the wrong game
							let ishitman3 = path.join("Retail").join("HITMAN3.exe").is_file();

							let framework_already_installed =
								path.join("Simple Mod Framework").is_dir();

							if !framework_already_installed
								&& subfolder_retail && subfolder_runtime
								&& ishitman3 && !self
								.valid_game_folders
								.iter()
								.any(|(x, y)| *x == path && *y == username)
							{
								self.valid_game_folders.push((path.to_owned(), username));
							}
						}

						if !self.valid_game_folders.is_empty() {
							self.selected_game_folder = Some(0);
						}

						self.performed_automatic_check = true;
					}

					if !self.valid_game_folders.is_empty() {
						if self.valid_game_folders.len() == 1 {
							ui.label(
								RichText::from("✅ Game folder found automatically").size(7.0)
							);
						} else {
							ComboBox::from_label(
								RichText::from("Select your game folder").size(7.0)
							)
							.selected_text(
								RichText::from(if let Some(x) = self.selected_game_folder {
									String::from(
										self.valid_game_folders
											.get(x)
											.context("selected_game_folder")?
											.0
											.to_string_lossy()
									)
								} else {
									"".to_owned()
								})
								.size(7.0)
							)
							.width(200.0)
							.show_ui(ui, |ui| {
								for (ind, (folder, _)) in self.valid_game_folders.iter().enumerate()
								{
									ui.selectable_value(
										&mut self.selected_game_folder,
										Some(ind),
										RichText::from(folder.to_string_lossy()).size(7.0)
									);
								}
							});
						}
					} else {
						ui.label(
							RichText::from(
								"It doesn't look like HITMAN 3 is installed anywhere (that, or \
								 every version already has the framework installed). Make sure \
								 you're trying to install the framework on a copy of HITMAN 3 \
								 installed via Steam, Epic Games Launcher/Legendary or the Xbox \
								 app; if you can't fix this, contact Atampy26 on Hitman Forum \
								 (note that this does not say Nexus Mods)."
							)
							.size(7.0)
						);
					}

					Ok(())
				})();

				if let Err(x) = x {
					self.error = Some(format!("{x:?}"));
				}

				ui.add_space(5.0);

				if self.selected_game_folder.is_some() {
					ui.label("Ready to install the framework?");

					if let Some(download_size) = self.download_size {
						ui.label(
							RichText::from(format!(
								"This will download {} of data.",
								human_bytes(download_size)
							))
							.size(8.0)
						);

						ui.horizontal_wrapped(|ui| {
							if let Some(selected_game_folder) = self.selected_game_folder {
								if self.download_promise.is_none()
									&& ui
										.button(RichText::from("Install the framework").size(7.0))
										.clicked()
								{
									self.download_promise.get_or_insert_with(|| {
										let ctx = ctx.clone();
										let (sender, promise) = Promise::new();

										let request = ehttp::Request::get(FRAMEWORK_DOWNLOAD_URL);

										ehttp::fetch(request, move |response| {
											let data = response.map(|x| x.bytes);
											sender.send(data);
											ctx.request_repaint();
										});

										promise
									});
								}

								let x = (|| -> anyhow::Result<()> {
									ui.label(
										RichText::from(
											self.valid_game_folders
												.get(selected_game_folder)
												.context("selected_game_folder")?
												.0
												.to_str()
												.context("game folder to_str")?
										)
										.size(7.0)
									);

									ui.label(
										RichText::from(
											if let Some(s) = &self
												.valid_game_folders
												.get(selected_game_folder)
												.context("selected_game_folder")?
												.1
											{
												format!("✅ Hello, {}!", s)
											} else {
												if self.valid_game_folders.len() == 1 {
													"✅ Game folder found automatically"
												} else {
													"✅ Game folder selected"
												}
												.to_owned()
											}
										)
										.size(7.0)
									);

									Ok(())
								})();

								if let Err(x) = x {
									self.error = Some(format!("{x:?}"));
								}
							}
						});

						ui.add_space(5.0);

						if let Some(promise) = &self.download_promise {
							match promise.ready() {
								None => {
									ui.spinner();
								}
								Some(data) => {
									if !self.installation_done {
										match data {
											Ok(data) => {
												match (|| -> anyhow::Result<()> {
													extract(
														Cursor::new(data),
														&self
															.valid_game_folders
															.get(
																self.selected_game_folder.context(
																	"selected_game_folder"
																)?
															)
															.context("game folder index")?
															.0
															.join("Simple Mod Framework"),
														false
													)?;

													ShellLink::new(
														self.valid_game_folders
															.get(
																self.selected_game_folder.context(
																	"selected_game_folder"
																)?
															)
															.context("game folder index")?
															.0
															.join("Simple Mod Framework")
															.join("Mod Manager")
															.join("Mod Manager.exe")
															.to_str()
															.context("linktarget to_str")?
													)?
													.create_lnk(
														home_dir()
															.context("home dir")?
															.join("AppData")
															.join("Roaming")
															.join("Microsoft")
															.join("Windows")
															.join("Start Menu")
															.join("Programs")
															.join("Simple Mod Framework.lnk")
													)?;

													Ok(())
												})()
												.context("Extracting/creating link")
												{
													Ok(_) => {
														self.installation_done = true;
													}

													Err(e) => {
														self.error = Some(format!("{e:?}"));
													}
												};
											}

											Err(e) => {
												self.error = Some(format!("{e:?}"));
											}
										}
									}

									if self.installation_done {
										ui.add_space(4.0);

										ui.label("Installation done!");

										ui.label(
											RichText::from(
												"You can close this window; a shortcut has been \
												 added to the Start menu."
											)
											.size(8.0)
										);
									}
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
}
