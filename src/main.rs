use grammers_client::types::Chat;
use grammers_client::{Client, Config, InitParams, SignInError, Update};
use grammers_mtsender::InvocationError;
use grammers_session::Session;
use grammers_tl_types as tl;
use log::{self, error, info, warn};
use simple_logger::SimpleLogger;
use std::env;
use std::io::{self, BufRead as _, Write as _};
use tokio::{runtime, task};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const SESSION_FILE: &str = "app.session";

fn prompt(message: &str) -> Result<String> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    stdout.write_all(message.as_bytes())?;
    stdout.flush()?;

    let stdin = io::stdin();
    let mut stdin = stdin.lock();

    let mut line = String::new();
    stdin.read_line(&mut line)?;
    Ok(line)
}

async fn async_main() -> Result<()> {
    SimpleLogger::new()
        .with_level(log::LevelFilter::Warn)
        .init()
        .unwrap();

    let api_id = env!("TG_ID").parse().expect("TG_ID invalid");
    let api_hash = env!("TG_HASH").to_string();

    println!("Connecting to Telegram...");
    let mut client = Client::connect(Config {
        session: Session::load_file_or_create(SESSION_FILE)?,
        api_id,
        api_hash: api_hash.clone(),
        params: InitParams {
            proxy_url: Some("socks5://127.0.0.1:1086".to_string()),
            ..Default::default()
        },
    })
    .await?;
    println!("Connected!");

    // If we can't save the session, sign out once we're done.
    let mut sign_out = false;

    if !client.is_authorized().await? {
        println!("Signing in...");
        let phone = prompt("Enter your phone number (international format): ")?;
        let token = client.request_login_code(&phone, api_id, &api_hash).await?;
        let code = prompt("Enter the code you received: ")?;
        let signed_in = client.sign_in(&token, &code).await;
        match signed_in {
            Err(SignInError::PasswordRequired(password_token)) => {
                // Note: this `prompt` method will echo the password in the console.
                //       Real code might want to use a better way to handle this.
                let hint = password_token.hint().unwrap();
                let prompt_message = format!("Enter the password (hint {}): ", &hint);
                let password = prompt(prompt_message.as_str())?;

                client
                    .check_password(password_token, password.trim())
                    .await?;
            }
            Ok(_) => (),
            Err(e) => panic!("{}", e),
        };
        println!("Signed in!");
        match client.session().save_to_file(SESSION_FILE) {
            Ok(_) => {}
            Err(e) => {
                println!(
                    "NOTE: failed to save the session, will sign out when done: {}",
                    e
                );
                sign_out = true;
            }
        }
    }

    // Obtain a `ClientHandle` to perform remote calls while `Client` drives the connection.
    //
    // This handle can be `clone()`'d around and freely moved into other tasks, so you can invoke
    // methods concurrently if you need to. While you do this, the single owned `client` is the
    // one that communicates with the network.
    //
    // The design's annoying to use for trivial sequential tasks, but is otherwise scalable.
    let mut client_handle = client.clone();
    let network_handle = task::spawn(async move { client.run_until_disconnected().await });

    while let Some(update) = client_handle.next_update().await? {
        if let Update::NewMessage(message) = update {
            let _msg_id = message.id();
            let _input_peer = message.chat().pack().to_bytes();
            match message.chat() {
                Chat::Group(_group) => {
                    if let Some(Chat::User(user)) = message.sender() {
                        // if !user.0.min {
                        let input_user = user.pack().try_to_input_user().unwrap();
                        match get_full_user(client_handle.clone(), input_user).await {
                            Ok(_user) => {
                                warn!("get_full_user success");
                            }
                            Err(error) => error!("{}", error.to_string()),
                        }
                        // }
                    }
                }
                _ => {}
            }
        }
    }

    if sign_out {
        // TODO revisit examples and get rid of "handle references" (also, this panics)
        drop(client_handle.sign_out_disconnect().await);
    }

    network_handle.await??;
    Ok(())
}

async fn get_full_user(
    client: Client,
    id: tl::enums::InputUser,
) -> std::result::Result<tl::enums::users::UserFull, InvocationError> {
    client
        .invoke(&tl::functions::users::GetFullUser { id })
        .await
}

#[allow(dead_code)]
async fn get_users(
    client: Client,
    id: Vec<tl::enums::InputUser>,
) -> std::result::Result<Vec<tl::enums::User>, InvocationError> {
    client.invoke(&tl::functions::users::GetUsers { id }).await
}

fn main() -> Result<()> {
    runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async_main())
}
