# lobby-is-up
A Discord bot written in rust. 
The bot continuously monitors aoe2.net, and keeps the state of all active aoe2 lobbies.
The bot provides a slash command to query the state of a lobby. It updates the players in the lobby, in real time by editing the discord embed.
Example: `/lobby lobby_id:aoe2de://0/230389981`

## Setup
1. Bot requires following environment variables to be set:
    - `DISCORD_TOKEN`: Discord bot token
    - `GUILD_ID`: The server id where the bot is added

Invite: `https://discord.com/api/oauth2/authorize?client_id=<CLIENT_ID>&permissions=84992&scope=applications.commands%20bot`

## Docker
1. Build the docker image: `docker build -t lobby-is-up .`
2. Run the image: `docker run -e DISCORD_TOKEN -e GUILD_ID lobby-is-up` (Assuming the environment variables are set)