use std::{
	collections::HashMap,
	fs,
	io::Cursor,
	os::windows::process::CommandExt,
	path::{Path, PathBuf},
	process::Command
};

use base64::{engine::general_purpose, Engine};
use dirs::home_dir;
use egui::{CentralPanel, Color32, Context, RichText};
use human_bytes::human_bytes;
use ini::Ini;
use mslnk::ShellLink;
use poll_promise::Promise;
use registry::{Data, Hive, Security};
use rfd::FileDialog;
use serde::Deserialize;
use serde_json::Value;
use zip_extract::extract;

// This is just so that rustfmt doesn't completely stop formatting the codebase
// (it has an issue with print width that causes it to not format the whole function)
static FRAMEWORK_DOWNLOAD_URL: &str =
	"https://github.com/atampy25/simple-mod-framework/releases/latest/download/Release.zip";

pub struct App {
	game_folder: PathBuf,
	download_size: f64,
	download_promise: Option<Promise<Result<Vec<u8>, String>>>,
	installation_done: bool,
	error: Option<String>,
	performed_automatic_check: bool,
	automatic_check_result: bool,
	once_check_result: FrameworkCheckResult,
	username: Option<String>
}

enum FrameworkCheckResult {
	AlreadyInstalled,
	InvalidGameFolder,
	ValidFolder,
	NotComplete
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
		App {
			game_folder: PathBuf::new(),
			download_size: if let Ok(data) = reqwest::blocking::Client::new()
				.head(FRAMEWORK_DOWNLOAD_URL)
				.send()
			{
				data.headers()
					.get("Content-Length")
					.unwrap()
					.to_str()
					.unwrap()
					.parse()
					.unwrap()
			} else {
				-1.0
			},
			download_promise: None,
			installation_done: false,
			error: None,
			performed_automatic_check: false,
			automatic_check_result: false,
			once_check_result: FrameworkCheckResult::NotComplete,
			username: None
		}
	}
}

impl Default for App {
	fn default() -> Self {
		Self::new()
	}
}

impl eframe::App for App {
	fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
		ctx.set_pixels_per_point(3.0);

		if let Some(error) = &self.error {
			CentralPanel::default().show(ctx, |ui| {
				ui.label(RichText::from("Welcome to the Simple Mod Framework!").strong());

				ui.label(
					"It seems a critical error has occurred. Try restarting the installation \
					 process, or give Atampy26 the following error message on Hitman Forum."
				);

				ui.label(RichText::from(error).size(7.0));
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
						 may need to enable the \"Advanced management features\" option to \
						 manually select where to install the game. When that's done, you can \
						 select the Content folder as the game folder here."
					)
					.size(8.0)
				);

				let mut set_game_path = false;

				ui.horizontal_wrapped(|ui| {
					if !self.performed_automatic_check {
						let mut check_paths = vec![];

						let legendary_installed_path =
							Path::new(&std::env::var("USERPROFILE").unwrap())
								.join(".config")
								.join("legendary")
								.join("installed.json");

						// Check for a Legendary install
						if legendary_installed_path.exists() {
							let legendary_installed_data: Value = serde_json::from_slice(
								&fs::read(legendary_installed_path).unwrap()
							)
							.unwrap();

							if let Some(data) = legendary_installed_data.get("Eider") {
								check_paths.push((
									PathBuf::from(
										data.get("install_path").unwrap().as_str().unwrap()
									),
									Some(
										serde_json::from_slice::<Value>(
											&fs::read(
												Path::new(&std::env::var("USERPROFILE").unwrap())
													.join(".config")
													.join("legendary")
													.join("user.json")
											)
											.unwrap()
										)
										.unwrap()
										.get("displayName")
										.unwrap()
										.as_str()
										.unwrap()
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
									for entry in fs::read_dir(d.to_string_lossy())
										.unwrap()
										.filter_map(|x| x.ok())
										.filter(|x| x.file_type().unwrap().is_file())
									{
										let manifest_data: Value = serde_json::from_slice(
											&fs::read(entry.path()).unwrap()
										)
										.unwrap();

										if manifest_data.get("AppName").unwrap().as_str().unwrap()
											== "Eider"
										{
											let mut username = None;

											if Path::new(&std::env::var("LOCALAPPDATA").unwrap())
												.join("EpicGamesLauncher")
												.join("Saved")
												.join("Config")
												.join("Windows")
												.join("GameUserSettings.ini")
												.exists()
											{
												if let Some(x) = Ini::load_from_file(
													Path::new(
														&std::env::var("LOCALAPPDATA").unwrap()
													)
													.join("EpicGamesLauncher")
													.join("Saved")
													.join("Config")
													.join("Windows")
													.join("GameUserSettings.ini")
												)
												.unwrap()
												.section(Some("Offline"))
												.and_then(|x| x.get("Data"))
												{
													username = Some(
														serde_json::from_slice::<Value>(
															&general_purpose::STANDARD
																.decode(x)
																.unwrap()
														)
														.unwrap()
														.get(0)
														.unwrap()
														.get("DisplayName")
														.unwrap()
														.as_str()
														.unwrap()
														.to_owned()
													);
												};
											}

											check_paths.push((
												PathBuf::from(
													manifest_data
														.get("InstallLocation")
														.unwrap()
														.as_str()
														.unwrap()
												),
												username
											));
										}
									}
								}

								Ok(_) => {
									self.error = Some(
										"Registry key ModSdkMetadataDir was not string".to_owned()
									);
									return;
								}

								Err(err) => {
									self.error = Some(err.to_string());
									return;
								}
							}
						}

						// Check for a Steam install
						if let Ok(hive) =
							Hive::CurrentUser.open(r#"Software\Valve\Steam"#, Security::Read)
						{
							match hive.value("SteamPath") {
								Ok(Data::String(d)) => {
									match fs::read_to_string(
										Path::new(&d.to_string_lossy())
											.join("config")
											.join("libraryfolders.vdf")
									) {
										Ok(s) => {
											let folders: HashMap<String, SteamLibraryFolder> =
												keyvalues_serde::from_str(&s).unwrap();

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
															)
															.unwrap()
														)
														.unwrap();

													check_paths.push((
														Path::new(&folder.path)
															.join("steamapps")
															.join("common")
															.join("HITMAN 3"),
														Some(
															users
																.values()
																.find(|x| x.most_recent)
																.unwrap()
																.persona_name
																.to_owned()
														)
													));
												}
											}
										}

										Err(err) => {
											self.error = Some(err.to_string());
											return;
										}
									};
								}

								Ok(_) => {
									self.error =
										Some("Registry key SteamPath was not string".to_owned());
									return;
								}

								Err(err) => {
									self.error = Some(err.to_string());
									return;
								}
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
							if Path::new(&path.join("Retail")).is_dir()
								&& (Path::new(&path.join("Runtime")).is_dir()
									|| Path::new(&path.join("Retail").join("Runtime")).is_dir())
								&& !path.join("Simple Mod Framework").is_dir()
							{
								self.game_folder = path;
								self.username = username;
								self.automatic_check_result = true;
							}
						}

						self.performed_automatic_check = true;
					}

					if self.game_folder.to_str().unwrap() != "" {
						ui.label(RichText::from(self.game_folder.to_str().unwrap()).size(7.0));

						if let FrameworkCheckResult::NotComplete = self.once_check_result {
							// Game folder has Retail
							let subfolder_retail = self.game_folder.join("Retail").is_dir();

							// Game folder has Runtime or Retail/Runtime
							let subfolder_runtime = self.game_folder.join("Runtime").is_dir()
								|| self.game_folder.join("Retail").join("Runtime").is_dir();

							// User is not trying to install the framework on the wrong game
							let ishitman3 = self
								.game_folder
								.join("Retail")
								.join("HITMAN3.exe")
								.is_file();

							let framework_already_installed =
								self.game_folder.join("Simple Mod Framework").is_dir();

							if framework_already_installed {
								self.once_check_result = FrameworkCheckResult::AlreadyInstalled;
							} else if subfolder_retail && subfolder_runtime && ishitman3 {
								self.once_check_result = FrameworkCheckResult::ValidFolder;
							} else {
								self.once_check_result = FrameworkCheckResult::InvalidGameFolder;
							}
						}

						match self.once_check_result {
							FrameworkCheckResult::AlreadyInstalled => {
								ui.label(
									RichText::from("❌ Framework already installed here").size(7.0)
								);
							}

							FrameworkCheckResult::ValidFolder => {
								if self.automatic_check_result {
									ui.label(
										RichText::from(if let Some(s) = &self.username {
											format!("✅ Hello, {}!", s)
										} else {
											"✅ Game folder found automatically".to_owned()
										})
										.size(7.0)
									);
								} else {
									ui.label(RichText::from("✅ Game folder selected").size(7.0));
								}
								set_game_path = true;
								return;
							}

							FrameworkCheckResult::InvalidGameFolder => {
								ui.label(RichText::from("❌ Not a HITMAN 3 folder").size(7.0));
							}

							FrameworkCheckResult::NotComplete => {}
						}
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
							if let Some(first_subfolder) = fs::read_dir(&folder).unwrap().next() {
								// Folder has contents
								if first_subfolder
									.as_ref()
									.unwrap()
									.path()
									.join("Retail")
									.is_dir()
								{
									// Subfolder exists with Retail inside it (i.e. user has selected containing folder instead of game folder)
									folder = first_subfolder.unwrap().path();
								}
							}

							if let Some(parent_folder) = folder.parent() {
								// Folder has a parent
								if parent_folder.join("Retail").is_dir() {
									// Parent folder contains a Retail folder (i.e. user has selected Retail/Runtime instead of game folder)
									folder = parent_folder.to_owned();
								}
							}

							self.game_folder = folder;

							self.once_check_result = FrameworkCheckResult::NotComplete;
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

								let request = ehttp::Request::get(FRAMEWORK_DOWNLOAD_URL);

								ehttp::fetch(request, move |response| {
									let data = response.map(|x| x.bytes);
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
										match data {
											Ok(data) => {
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

											Err(e) => {
												ui.label(
													RichText::from(e)
														.color(Color32::from_rgb(200, 50, 50))
														.size(8.0)
												);
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
