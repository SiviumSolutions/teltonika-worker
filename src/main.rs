use std::{fs::{self, OpenOptions}, io::{Seek, SeekFrom}, thread, time::Duration};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::process::Command;
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

const COUNTER_FILE: &str = "/storage/data/values.store";
const DEFAULT_SCALE: f32 = 1.0;
const DEFAULT_STATE: f32 = 4.0;
const DEFAULT_COEF: f32 = 0.625;

fn read_float_from_file(file: &mut fs::File, offset: u64) -> Result<f32, std::io::Error> {
    file.seek(SeekFrom::Start(offset))?;
    file.read_f32::<BigEndian>()
}

fn write_float_to_file(file: &mut fs::File, offset: u64, value: f32) -> Result<(), std::io::Error> {
    file.seek(SeekFrom::Start(offset))?;
    file.write_f32::<BigEndian>(value)
}

fn initialize_file() -> Result<(), std::io::Error> {
    let mut file = OpenOptions::new().read(true).write(true).create(true).open(COUNTER_FILE)?;
    let metadata = fs::metadata(COUNTER_FILE)?;
    let size = metadata.len();

    if size < 4 {
        write_float_to_file(&mut file, 0, 0.0)?;
    }
    if size < 8 {
        write_float_to_file(&mut file, 4, DEFAULT_SCALE)?;
    }
    if size < 12 {
        write_float_to_file(&mut file, 8, 0.0)?;
    }
    if size < 16 {
        write_float_to_file(&mut file, 12, DEFAULT_STATE)?;
    }
    if size < 20 {
        write_float_to_file(&mut file, 16, DEFAULT_COEF)?;
    }
    Ok(())
}

fn monitor_dwi0(scale_dwi: Arc<Mutex<f32>>) {
    let mut previous_value_dwi: Option<String> = None;
    loop {
        let output = match Command::new("ubus")
            .args(&["call", "ioman.dwi.dwi0", "status"])
            .output() {
                Ok(output) => output,
                Err(e) => {
                    eprintln!("Failed to execute command: {:?}", e);
                    thread::sleep(Duration::from_millis(100));
                    continue;
                }
            };

        let result_str = String::from_utf8_lossy(&output.stdout);
        let data: Value = serde_json::from_str(&result_str).unwrap_or_else(|_| serde_json::json!({}));
        let current_value_dwi = data["value"].as_str().unwrap_or("");

        if current_value_dwi == "1" && previous_value_dwi.as_deref() != Some("1") {
            let mut file = match OpenOptions::new().read(true).write(true).open(COUNTER_FILE) {
                Ok(file) => file,
                Err(e) => {
                    eprintln!("Failed to open file: {:?}", e);
                    thread::sleep(Duration::from_millis(100));
                    continue;
                }
            };
            let scale = scale_dwi.lock().unwrap();
            if let Ok(counter_dwi) = read_float_from_file(&mut file, 0) {
                let new_counter_dwi = counter_dwi + *scale;
                if let Err(e) = write_float_to_file(&mut file, 0, new_counter_dwi) {
                    eprintln!("Failed to write to file: {:?}", e);
                } else {
                    println!("dwi0 - New data: {} with scale {} at {:?}", new_counter_dwi, *scale, chrono::Local::now());
                }
            } else {
                eprintln!("Failed to read from file.");
            }
        }

        previous_value_dwi = Some(current_value_dwi.to_string());
        thread::sleep(Duration::from_millis(100));
    }
}

fn monitor_acl0(state_acl0: Arc<Mutex<f32>>, coef_acl0: Arc<Mutex<f32>>) {
    loop {
        let output = match Command::new("ubus")
            .args(&["call", "ioman.acl.acl0", "status"])
            .output() {
                Ok(output) => output,
                Err(e) => {
                    eprintln!("Failed to execute command: {:?}", e);
                    thread::sleep(Duration::from_secs(1));
                    continue;
                }
            };

        let result_str = String::from_utf8_lossy(&output.stdout);
        let data: Value = serde_json::from_str(&result_str).unwrap_or_else(|_| serde_json::json!({}));
        let current_value_acl: f32 = data["value"].as_str().unwrap_or("0").parse().unwrap_or(0.0);

        let mut file = match OpenOptions::new().read(true).write(true).open(COUNTER_FILE) {
            Ok(file) => file,
            Err(e) => {
                eprintln!("Failed to open file: {:?}", e);
                thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        let state = *state_acl0.lock().unwrap();
        let coef = *coef_acl0.lock().unwrap();
        let adjusted_value = ((current_value_acl - state) * coef).max(0.0);
        if let Err(e) = write_float_to_file(&mut file, 8, adjusted_value) {
            eprintln!("Failed to write to file: {:?}", e);
        } else {
            println!("acl0 - New pressure value: {} with coef {} and state {} at {:?}", adjusted_value, coef, state, chrono::Local::now());
        }

        thread::sleep(Duration::from_secs(1));
    }
}

fn main() {
    if let Err(e) = initialize_file() {
        eprintln!("Failed to initialize file: {:?}", e);
        return;
    }

    let scale_dwi = Arc::new(Mutex::new(read_float_from_file(&mut fs::File::open(COUNTER_FILE).unwrap(), 4).unwrap()));
    let state_acl0 = Arc::new(Mutex::new(read_float_from_file(&mut fs::File::open(COUNTER_FILE).unwrap(), 12).unwrap()));
    let coef_acl0 = Arc::new(Mutex::new(read_float_from_file(&mut fs::File::open(COUNTER_FILE).unwrap(), 16).unwrap()));

    let scale_dwi_clone = Arc::clone(&scale_dwi);
    let state_acl0_clone = Arc::clone(&state_acl0);
    let coef_acl0_clone = Arc::clone(&coef_acl0);

    let handle_dwi0 = thread::spawn(move || {
        monitor_dwi0(scale_dwi_clone);
    });

    let handle_acl0 = thread::spawn(move || {
        monitor_acl0(state_acl0_clone, coef_acl0_clone);
    });

    handle_dwi0.join().unwrap();
    handle_acl0.join().unwrap();
}
