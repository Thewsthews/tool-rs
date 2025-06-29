use infobip_sdk::api::whatsapp::WhatsAppClient;
use infobip_sdk::configuration::Configuration;
use serde::{Deserialize, Serialize};
use std::env;
use tokio::sync::mpsc;
use warp::Filter;
use dotenv::dotenv;
use log::{error, info};

// This is the configuration struct for environment variables
mod some_module{
    use serde::Deserialize;

    #[derive(Debug, Deserialize, Clone)]
    pub struct Config{
        pub infobip_api_key: String,
        pub infobip_base_url: String,
        pub whatsapp_phone_number_id: String,
        pub trigger_word: String,
        pub recipient_phone_number: String,
    }
}

// Incoming wozap payloaddd!
#[derive(Debug, Deserialize)]
struct WhatsAppMessage {
    from: String,
    text: Option<String>,
}

// This is the VCard struct for the contact info
#[derive(Debug,Serialize)]
struct VCard{
    first_name: String,
    last_name: String,
    phone_number: String,
}

//Initializing the logging
fn init_logging() {
    env_logger::init();
}

//Load configuration from environment variables
fn load_config() -> some_module::Config{
    some_module::Config{
        infobip_api_key: env::var("INFOBIP_API_KEY").expect("INFOBIP_API_KEY must be set"),
        infobip_base_url: env::var("INFOBIP_BASE_URL").expect("INFOBIP_BASE_URL must be set"),
        whatsapp_phone_number_id: env::var("WHATSAPP_PHONE_NUMBER_ID").expect("WHATSAPP_PHONE_NUMBER_ID must be set"),
        trigger_word: env::var("TRIGGER_WORD").unwrap_or("addcontact".to_string()),
        recipient_phone_number: env::var("RECIPIENT_PHONE_NUMBER").expect("RECIPIENT_PHONE_NUMBER must be set"),
    }
}

//Generate the vCard content
fn generate_vcard(contact: &VCard) -> String{
    format!(
        "BEGIN:VCARD\nVERSION:3.0\nN:{};{}\nTEL;TYPE=CELL:{}\nEND:VCARD",
        contact.last_name, contact.first_name, contact.phone_number
    )
}

// Define a local Message and Content struct for WhatsApp sending
#[derive(Debug, Serialize, Default)]
struct Message {
    from: String,
    to: String,
    content: Content,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "text")]
enum Content {
    #[serde(rename = "text")]
    Text(String),
}

impl Default for Content {
    fn default() -> Self {
        Content::Text(String::new())
    }
}

async fn send_vcard(client: &WhatsAppClient, config: &some_module::Config, vcard: &str, recipient: &str) -> Result<(), Box<dyn std::error::Error>>{
    // the sdk might not provide native support for certain functionalites
    // Refer to official crate for more clarification

    let message = format!("Here is the contact vCard:\n{}", vcard);

    use infobip_sdk::model::whatsapp::SendTextRequestBody;

    use infobip_sdk::model::whatsapp::TextContent;

    let request_body = SendTextRequestBody {
        from: config.whatsapp_phone_number_id.clone(),
        to: recipient.to_string(),
        content: TextContent {
            text: message.clone(),
            preview_url: Some(false),
        },
        ..Default::default()
    };

    match client
        .send_text(request_body)
        .await
    {
        Ok(_) => {
            info!("vCard sent successfully to {}", recipient);
            Ok(())
        }
        Err(e) => {
            error!("Failed to send vCard: {}", e);
            Err(Box::new(e))
        }
    }
}

// Webhook handler for incoming WhatsApp messages
async fn handle_webhook(
    message: WhatsAppMessage,
    config: some_module::Config,
    client: WhatsAppClient,
) -> Result<impl warp::Reply, warp::Rejection>{
    info!("Received message from {}: {:?}", message.from, message.text);

    let trigger_word = config.trigger_word.to_lowercase();
    let message_text = message.text.unwrap_or_default().to_lowercase();

    if message_text.contains(&trigger_word){
        info!("Trigger word '{}' detected from {}", trigger_word, message.from);

        //example contact
        let contact = VCard{
            first_name: "John".to_string(),
            last_name: "Doe".to_string(),
            phone_number: "1234567890".to_string(),
        };

        let vcard = generate_vcard(&contact);
        if let Err(e) = send_vcard(&client, &config, &vcard, &config.recipient_phone_number).await{
            error!("Error sending vCard: {}", e);
            return Ok(warp::reply::with_status("Failed to send vCard", warp::http::StatusCode::INTERNAL_SERVER_ERROR));
        }
    }
    Ok(warp::reply::with_status("Message processed", warp::http::StatusCode::OK))
}

#[tokio::main]
async fn main(){
    init_logging();
    dotenv().ok();
    let config = load_config();
    info!("Starting WhatsApp contact adder with trigger word: {}", config.trigger_word);

    //Initializes infobip wozap client
    let mut configuration = Configuration::from_env_api_key()
        .expect("Failed to load Infobip configuration from environment");
    // Use the set_base_url method if available, otherwise construct Configuration manually
    configuration = configuration.with_base_url(config.infobip_base_url.clone());
    let client = WhatsAppClient::with_configuration(configuration);
    
    let (tx, mut rx) = mpsc::channel::<WhatsAppMessage>(100);

    //Spawn a task to process messages with rate limiting
    let client_clone = client.clone();
    let config_clone = config.clone();
    tokio::spawn(async move{
        while let Some (message) = rx.recv().await{
            if let Err(e) = handle_webhook(message, config_clone, client_clone).await{
                error!("Error processing webhook: {:?}", e);
            }

            // rate limiting of one sec between messages
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    });
    let webhook = warp::post()
        .and(warp::path("webhook"))
        .and(warp::body::json())
        .and(warp::any().map(move || config.clone()))
        .and(warp::any().map(move || client.clone()))
        .and_then(handle_webhook);

    info!("WhatsApp contact adder is running...");
    warp::serve(webhook).run(([0, 0, 0, 0], 8080)).await;
}