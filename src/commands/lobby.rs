use once_cell::sync::Lazy;
use regex::Regex;
use serenity::builder::CreateApplicationCommand;
use serenity::model::application::interaction::application_command::CommandDataOption;
use serenity::model::prelude::command::CommandOptionType;

static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^aoe2de://0/\d+$").unwrap());

pub fn run(options: &[CommandDataOption]) -> String {
    match extract_lobby_id(options){
        Some(lobby_id) => format!("Lobby ID: {}", lobby_id),
        None => "Invalid lobby id".to_string()
    }
}

fn extract_lobby_id(options: &[CommandDataOption]) -> Option<String>{
    if let Some(command_application) = options.first(){
        if let Some(value) = &command_application.value{
            if let Some(lobby_id) = value.as_str(){
                let lobby_id = lobby_id.trim();
                if RE.is_match(lobby_id){
                    return Some(lobby_id.to_string());
                }
            }
        }
    }
    None
}

pub fn register(command: &mut CreateApplicationCommand) -> &mut CreateApplicationCommand {
    command
        .name("lobby")
        .description("Aoe2 Lobby ID")
        .create_option(|option| {
            option
                .name("lobby_id")
                .description("The lobby id(ex: aoe2de://0/123456789)")
                .kind(CommandOptionType::String)
                .required(true)
        })
}
