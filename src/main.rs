use std::io;
use std::io::{BufReader, Write, ErrorKind};
use std::fs;
use std::fs::{File, OpenOptions};
use std::path::{PathBuf, Path};
use std::thread;
use std::time::{SystemTime, Duration};
use std::process;
use std::collections::HashMap;

use console::{Term, style};
use reqwest;
use chrono;
use walkdir::WalkDir;
use glob::glob;
use fs_extra;
use serde_json::Value;

mod settings;
use settings::Settings;

const STEAM_APP_LIST_URL: &str = "https://api.steampowered.com/ISteamApps/GetAppList/v2/";

const COLOR_DARK_GRAY: u8 = 244;
const COLOR_LIGHT_GRAY: u8 = 250;
const COLOR_WARNING_YELLOW: u8 = 143;

// 8 bit colors: https://jonasjacek.github.io/colors/
// TODO 20.09.2020: Download the appids.json instead of manually adding it.
// EDIT 27.10.2020: i actually did the thing! It nows downloads the appids.json file! :D
// NOTE 01.11.2020: This probably won't work out of the box on Linux, mostly because i have no idea how Steam's storage system works on Linux. Mild changes are probably required for it to work.
fn main() {
    match run() {
        Err(e) => {
            match e.kind() {
                ErrorKind::Other => println!("Heads up: '{}'", e),
                ErrorKind::NotFound => println!("NotFound Error: '{}'", e),
                _ => /*println!("Oh fiddlestricks, what now?")*/ println!("Error Detected: '{}'", e),
            }
    
            thread::sleep(Duration::from_secs(5));
            process::exit(1);
        },
        Ok(_) => process::exit(0),
    };
}

fn run() -> io::Result<()> {  
    let dir_app: PathBuf = {
        let exe = std::env::current_exe().unwrap();
        let parent = exe.parent().unwrap();
        parent.to_path_buf()
    };

    let dir_appids = &dir_app.join("appids.json");
    let dir_settings = &dir_app.join("settings.toml");

    let term = Term::stdout();
    term.hide_cursor()?;

    term.set_title("Steam Screenshot Backup | Rust Edition");

    // SETTINGS - START
    /*
    if !dir_settings.exists() {
        Settings::save(dir_settings.as_path(), &Settings::default());
        term.write_line("Settings file created!\nPlease edit and press ENTER to continue!")?;
        term.show_cursor()?;
        term.read_line()?;        
    } 
    */

    let m_settings: Settings = if dir_settings.exists() {
        if let Ok(settings) = Settings::load(dir_settings.as_path()) {
            // If the settings file exists, and it loaded properly; return it.
            settings
        }
        else {
            // If the settigns file exists, but it for some reason failed to load, take the user to the Settings prompt with a accompaning warning.
            term.write_line("Your settings file exists; but failed to load!\nPress ENTER to attempt to make a new one and continiue, or CTRL-C to exit.").unwrap();
            term.read_line().unwrap();
            settings_prompt(&term)
        }
    }
    else
    {
        // If no settings file exists; create one by going through and setting each variable step by step.
        settings_prompt(&term)
    };

    // Save the settings file in order to ensure that it is updated and properly formatted.
    Settings::save(&dir_settings, &m_settings);
    // SETTINGS - END

    // ARG PROCESSING - START
    let mut flag_noinput: bool = false;
    for x in std::env::args() {
        match x.as_str() {
            "-noinput" => {
                flag_noinput = true;
            },
            _ => {}
        }
    }
    // ARG PROCESSING - END

    // APP ID LIST - START

    if !m_settings.force_disable_update {
        let mut is_ready = false;
        while !is_ready { // Absolutelly dirty way of doing this, but you know what? It works, so screw it, i can't figure out a better way of doing it, so this'll work.
            if !dir_appids.exists() {
                term.write_line("AppID file is missing, attempting to download..")?;
                match reqwest::blocking::get(STEAM_APP_LIST_URL).unwrap().text() {
                    Ok(body) => {
                        match OpenOptions::new().read(true).write(true).create(true).open(dir_appids) {
                            Ok(mut f) => {
                                f.write_all(body.as_bytes()).unwrap(); 
                                term.write_line("AppID file downloaded and saved!")?;
                            },
                            Err(err) => {
                                panic!("Error Writing File: {:?}", err);
                            }
                        }
                    },
                    Err(err) => {
                        panic!("Error While Getting AppIDs file: {:?}", err);
                    }
                };
            }
            else {
                match fs::metadata(dir_appids) {
                    Ok(metadata) => {
                        match SystemTime::now().duration_since(metadata.modified().unwrap()) {
                            Ok(m) => { 
                                if chrono::Duration::from_std(m).unwrap() >= chrono::Duration::days(7) {
                                    term.write_line("AppID file outdated! Deleting..")?;
                                    fs::remove_file(dir_appids).unwrap();
                                }
                                else {                        
                                    is_ready = true;
                                }
                            },
                            Err(_) => {
                                panic!("SystemTime not valid time format!"); // Not sure when this will happen tbh, but its safe to be safe.
                            },
                        };
                    },
                    Err(_) => panic!("Metadata acquisition failed!")
                }
    
                
            }
        }
    }
    
    // Note: Fairly memory intensive in the beginning peaking at around 80mb but drops down to around 10mb when finished.
    let appid_map: HashMap<u32, String> = {
        let appids: Value = {
            let file_appids = File::open(dir_appids).unwrap();
            let appids_reader = BufReader::new(file_appids);

            serde_json::from_reader(appids_reader).unwrap()
        };

        let mut _map: HashMap<u32, String> = HashMap::new();
        let appid_length = appids["applist"]["apps"].as_array().unwrap().len();
    
        for i in 0..appid_length {
            let _appid = appids["applist"]["apps"][i]["appid"].as_i64().unwrap().clone() as u32;
            let _name = appids["applist"]["apps"][i]["name"].to_string();
            
            _map.insert(_appid, _name);
        }

        _map.insert(0, "Empty".to_string());

        _map
    };
    // APP ID LIST - END

    for entry in WalkDir::new(&m_settings.steam_folder).follow_links(false).into_iter() {
        let e = &entry.unwrap();
        if e.file_type().is_dir() && e.file_name().to_string_lossy() == "screenshots" {
            let folder_id: u32 = e.clone().path().parent().unwrap().file_name().unwrap().to_string_lossy().trim().parse().unwrap_or(0);

            if appid_map.contains_key(&folder_id) {
                let mut retreived_app_name = appid_map.get(&folder_id).unwrap().clone();
                retreived_app_name.retain(|c| !r#"[\/?:*""><|]+"#.contains(c)); // FILTER
                retreived_app_name = retreived_app_name.trim().to_string();

                term.write_line(format!("{}", style(format!("Found game '{0}' with AppID '{1}'", &retreived_app_name, &folder_id).as_str()).color256(COLOR_DARK_GRAY)).as_str())?;
                if !m_settings.disable_artifical_delay {
                    thread::sleep(Duration::from_millis(100));
                }
            
                if folder_id > 0 {
                    let target_path = &Path::new(&m_settings.target_folder).join(retreived_app_name);
                    if !target_path.exists() {
                        fs::create_dir_all(target_path).unwrap();
                    }
                    
                    let options = fs_extra::dir::CopyOptions::new();

                    for entry_img in glob(e.path().join("*.jpg").to_str().unwrap()).unwrap() {
                        if let Ok(img) = entry_img {

                            let target_file = target_path.join(img.file_name().unwrap());

                            let from_paths = vec![img];

                            if !target_file.exists() {                               
                                match fs_extra::copy_items(&from_paths, &target_path, &options) {
                                    Ok(_) => term.write_line(format!("{}", style(target_file.to_str().unwrap()).color256(COLOR_LIGHT_GRAY)).as_str())?,
                                    Err(_) => term.write_line(format!("{}", style(target_file.to_str().unwrap()).color256(COLOR_WARNING_YELLOW)).as_str())?, // Optimally this should spew out an error, but for now, i will only indicate by color that something is wrong.
                                }
                                
                                if !m_settings.disable_artifical_delay {
                                    thread::sleep(Duration::from_millis(50));
                                }                               
                            }  
                            
                            drop(from_paths);
                        }
                    }
                }
            }       
        }
    }

    if !flag_noinput {
        finish(&term);
    }

    Ok(())   
}

// Just a test function i made to test our borrowing.
fn finish(term: &Term) {
    term.write_line("Done! Press ENTER to exit!").unwrap();
    //term.show_cursor().unwrap();
    //drop(term.read_key());
    term.read_line().unwrap();
}

fn settings_prompt(term: &Term) -> Settings {  
    /// Nested function to make prompting the input easier and better ;) (Currently only for Strings)
    fn prompt_string(desc: &str, index: &str, default: String, term: &Term) -> String {
        term.write_line(format!("{} (Default: '{}')", desc, default).as_str()).unwrap();
        term.write_str(format!("{}", style(index).color256(COLOR_LIGHT_GRAY)).as_str()).unwrap();
        let input = term.read_line().unwrap_or(String::from(""));
        
        let value = if !input.is_empty() {
            input
        } else {
            default
        };
    
        term.write_line(format!("Using value: '{}'\n", value).as_str()).unwrap();

        value
    }

    let mut _settings = Settings::default();

    term.write_line("Settings file generator!\nPress ENTER to use the default value.\n").unwrap();

    _settings.steam_folder = prompt_string(
        "Path to Steam's userdata folder",
        ">> ", 
        Settings::default().steam_folder, 
        term);

    _settings.target_folder = prompt_string(
        "Path to a folder to copy the images to, example: 'C:/Users/MyName/Pictures/MySteamPictures/'",
        ">> ", 
        Settings::default().target_folder, 
        term);
    
    _settings
}