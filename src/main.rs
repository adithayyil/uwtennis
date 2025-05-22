use anyhow::Result;
use reqwest::Client;
use scraper::{Html, Selector};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::time::Duration;
use tokio::time;

const GET_URL: &str = "https://warrior.uwaterloo.ca/Program/GetProgramInstances";
const FILTER_URL: &str = "https://warrior.uwaterloo.ca/Program/FilterProgramInstances";

// Config struct to parse config.toml
#[derive(Debug, Deserialize)]
struct Config {
    interval_seconds: u64,
    ntfy_endpoint: String,
    program_ids: Vec<ProgramConfig>,
}

#[derive(Debug, Deserialize)]
struct ProgramConfig {
    id: String,
    name: String,
}

/// Default fields carried over in each appointment payload
fn default_fields() -> HashMap<&'static str, &'static str> {
    [
        ("RecurrenceInfo", ""),
        ("AppointmentType", "0"),
        ("Subject", ""),
        ("AllDay", "false"),
        ("ResourceId", ""),
        ("Status", "0"),
        ("ProductId", "00000000-0000-0000-0000-000000000000"),
        ("ProgramDescription", ""),
        ("ProgramInstanceId", "00000000-0000-0000-0000-000000000000"),
        ("NumberRegistered", "0"),
        ("NumberOnWaitlist", "0"),
        ("ClassSize", "12"),
        ("PortalURL", ""),
        ("InstructorFirstNameLastInitial", ""),
        ("IsInstructor", "false"),
        ("InstructorId", "00000000-0000-0000-0000-000000000000"),
        ("IsRecurring", "false"),
    ]
    .into()
}

#[derive(Debug, Deserialize, Clone)]
struct Appointment {
    #[serde(rename = "ID")]
    id: String,

    #[serde(rename = "StartDate")]
    start_date: String,

    #[serde(rename = "EndDate")]
    end_date: String,

    #[serde(rename = "Location")]
    location: String,

    #[serde(rename = "ProductName")]
    product_name: String,

    // ...
}

// Information about a specific appointment spot
#[derive(Debug, Clone)]
struct SpotInfo {
    program_name: String,
    product_name: String,
    date: String,
    time: String,
    spots: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load configuration
    let config_text = fs::read_to_string("config.toml")?;
    let config: Config = toml::from_str(&config_text)?;
    println!("🔄 Checking every {} seconds", config.interval_seconds);
    println!("🔔 Notifications will be sent to {}", config.ntfy_endpoint);
    println!("📋 Monitoring {} programs", config.program_ids.len());

    let client = Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:138.0)")
        .default_headers({
            let mut h = reqwest::header::HeaderMap::new();
            h.insert("Accept", "*/*".parse()?);
            h.insert(
                "Content-Type",
                "application/x-www-form-urlencoded; charset=UTF-8"
                    .parse()?,
            );
            h.insert("X-Requested-With", "XMLHttpRequest".parse()?);
            h.insert("Origin", "https://warrior.uwaterloo.ca".parse()?);
            h
        })
        .build()?;

    // Track previous spots to detect changes
    let mut previous_spots: HashMap<String, SpotInfo> = HashMap::new();
    
    // Main loop for periodic checking
    let mut interval = time::interval(Duration::from_secs(config.interval_seconds));
    loop {
        interval.tick().await;
        println!("⏱️ Checking for spot changes...");
        
        // Process each program ID concurrently
        let mut tasks = Vec::new();
        for program in &config.program_ids {
            let client = client.clone();
            let program_id = program.id.clone();
            let program_name = program.name.clone();
            let ntfy_endpoint = config.ntfy_endpoint.clone();
            
            let task = tokio::spawn(async move {
                match check_program(&client, &program_id, &program_name, &ntfy_endpoint).await {
                    Ok(current_spots) => current_spots,
                    Err(e) => {
                        eprintln!("Error checking program {}: {}", program_name, e);
                        HashMap::new()
                    }
                }
            });
            
            tasks.push(task);
        }
        
        // Wait for all tasks to complete and process results
        for task in tasks {
            if let Ok(current_spots) = task.await {
                for (key, spot_info) in current_spots {
                    // Check if spots have changed
                    if let Some(prev_info) = previous_spots.get(&key) {
                        if prev_info.spots != spot_info.spots {
                            println!("🔄 Change detected: {} ({}) on {} @ {} - {} → {}", 
                                spot_info.program_name, spot_info.product_name, 
                                spot_info.date, spot_info.time, 
                                prev_info.spots, spot_info.spots);
                            
                            // Send notification
                            let _ = send_notification(
                                &config.ntfy_endpoint,
                                &format!("Spot change: {}", spot_info.product_name),
                                &format!("{} ({}) on {} @ {}: {} → {}", 
                                    spot_info.program_name, spot_info.product_name, 
                                    spot_info.date, spot_info.time, 
                                    prev_info.spots, spot_info.spots)
                            ).await;
                        }
                    } else {
                        // First time seeing this appointment
                        println!("📌 New tracking: {} ({}) on {} @ {} - {}", 
                            spot_info.program_name, spot_info.product_name, 
                            spot_info.date, spot_info.time, spot_info.spots);
                    }
                    
                    // Update previous spots
                    previous_spots.insert(key, spot_info);
                }
            }
        }
    }
}

async fn check_program(
    client: &Client, 
    program_id: &str,
    program_name: &str,
    ntfy_endpoint: &str
) -> Result<HashMap<String, SpotInfo>> {
    // Fetch the data for this program
    let (appts, dates) = fetch_initial(client, program_id).await?;
    let mut current_spots = HashMap::new();
    
    for date_iso in dates {
        if let Some(appt) = appts.iter().find(|a| a.start_date.starts_with(&date_iso[..10])) {
            let spots = fetch_spots(client, appt, &date_iso).await?;
            let date = &date_iso[..10];
            let time = appt.start_date.split('T').nth(1).unwrap_or("").to_string();
            let key = format!("{}-{}-{}", program_id, date, appt.id);
            
            current_spots.insert(key, SpotInfo {
                program_name: program_name.to_string(),
                product_name: appt.product_name.clone(),
                date: date.to_string(),
                time,
                spots,
            });
        }
    }
    
    Ok(current_spots)
}

async fn send_notification(endpoint: &str, title: &str, message: &str) -> Result<()> {
    let client = Client::new();
    let response = client.post(endpoint)
        .header("Title", title)
        .body(message.to_string())
        .send()
        .await?;
        
    if response.status().is_success() {
        println!("✅ Notification sent successfully");
    } else {
        println!("❌ Failed to send notification: {}", response.status());
    }
    
    Ok(())
}

async fn fetch_initial(
    client: &Client,
    program_id: &str,
) -> Result<(Vec<Appointment>, Vec<String>)> {
    let res = client
        .get(GET_URL)
        .query(&[("programID", program_id)])
        .send()
        .await?
        .text()
        .await?;

    let document = Html::parse_document(&res);

    // Extract and parse appointments JSON
    let appt_sel = Selector::parse("input#ApptInfo").unwrap();
    let raw_appts = document
        .select(&appt_sel)
        .next()
        .and_then(|e| e.value().attr("value"))
        .ok_or_else(|| anyhow::anyhow!("Missing #ApptInfo input"))?;
    let appts: Vec<Appointment> = serde_json::from_str(raw_appts)?;

    // Extract and parse dates JSON
    let dates_sel = Selector::parse("input#hdnDates").unwrap();
    let raw_dates = document
        .select(&dates_sel)
        .next()
        .and_then(|e| e.value().attr("value"))
        .ok_or_else(|| anyhow::anyhow!("Missing #hdnDates input"))?;
    let dates: Vec<String> = serde_json::from_str(raw_dates)?;

    Ok((appts, dates))
}

async fn fetch_spots(
    client: &Client,
    appt: &Appointment,
    date_iso: &str,
) -> Result<String> {
    // Build form data
    let mut form = HashMap::new();
    let prefix = "appointments[0]";

    form.insert(format!("{prefix}[ID]"), appt.id.clone());
    form.insert(format!("{prefix}[StartDate]"), appt.start_date.clone());
    form.insert(format!("{prefix}[EndDate]"), appt.end_date.clone());
    form.insert(format!("{prefix}[Location]"), appt.location.clone());
    form.insert(
        format!("{prefix}[ProductName]"),
        appt.product_name.clone(),
    );

    for (k, v) in default_fields() {
        form.insert(format!("{prefix}[{k}]"), v.to_string());
    }

    // Date parts
    let date = &date_iso[..10];
    let parts: Vec<&str> = date.split('-').collect();
    let year = parts[0];
    let month = parts[1].trim_start_matches('0');
    let day = parts[2].trim_start_matches('0');

    // Use the program ID from the appointment context
    let program_id = match appt.id.split('-').next() {
        Some(id) => id,
        None => return Ok("Error: Invalid ID format".into()),
    };

    form.insert("programID".into(), program_id.into());
    form.insert("year".into(), year.into());
    form.insert("month".into(), month.into());
    form.insert("day".into(), day.into());

    // POST and parse response
    let res = client
        .post(FILTER_URL)
        .form(&form)
        .send()
        .await?
        .text()
        .await?;

    let document = Html::parse_document(&res);
    let sel_str = format!(
        "div[data-instance-appointmentid='{}'] .spots-tag",
        appt.id
    );
    let spot_sel = Selector::parse(&sel_str).unwrap();

    if let Some(el) = document.select(&spot_sel).next() {
        Ok(el.text().collect::<String>().trim().to_string())
    } else {
        Ok("N/A".into())
    }
}