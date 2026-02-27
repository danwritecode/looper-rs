pub mod read_file;
pub mod write_file;
pub mod list_directory;
pub mod grep;
pub mod find_files;

use crate::types::LooperTool;

pub fn get_tools() -> Vec<LooperTool> {
    vec![
        read_file::tool(),
        write_file::tool(),
        list_directory::tool(),
        grep::tool(),
        find_files::tool(),
    ]
}
