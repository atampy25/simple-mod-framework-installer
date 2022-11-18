use std::io;
use winres::WindowsResource;

fn main() -> io::Result<()> {
	WindowsResource::new().set_icon("icon.ico").compile()?;

	Ok(())
}
