use std::borrow::Cow;
use std::fmt::Write;

use clap::{App, AppSettings, Arg};

use crate::player::{Playlist, PlaylistLike};
use crate::Bot;

const COMMAND_PREFIX: char = ';';

pub async fn handle_message_event(bot: &Bot, ev: &mumble::event::Message) {
    let name: Cow<_> = match ev.actor {
        None => "<unknown>".into(),
        Some(r) => bot.client.get_user(r).unwrap().name().to_string().into(),
    };

    println!("{}: {}", name, ev.message);

    if let Some(msg) = ev.message.strip_prefix(COMMAND_PREFIX) {
        let msg = msg.trim();
        handle_command(bot, ev, msg).await;
    }
}

async fn handle_command(bot: &Bot, ev: &mumble::event::Message, msg: &str) {
    let cmds = tokenize(msg);

    for cmdline in cmds {
        let cmd = &*cmdline[0];
        let args = &cmdline[1..];

        match cmd {
            "skip" => skip(bot, ev, args).await,
            "pause" => pause(bot, ev, args).await,
            "play" => play(bot, ev, args).await,
            "list" => list(bot, ev, args).await,
            "new" => new(bot, ev, args).await,
            "newsub" => newsub(bot, ev, args).await,
            "quit" => quit(bot, ev, args).await,
            _ => {}
        }
    }
}

fn app_for_command(name: &'static str) -> App {
    App::new(name)
        .setting(AppSettings::DisableVersionFlag)
        .setting(AppSettings::NoBinaryName)
}

macro_rules! unwrap_matches {
    ($matches:ident, $bot:expr, $ev:expr) => {
        let $matches = match $matches {
            Ok(v) => v,
            Err(e) => {
                let text = format!("{}", e).replace('&', "&amp;").replace('<', "&lt;");
                $bot.client
                    .respond(&$ev, &format!("<pre>{}</pre>", text))
                    .await;
                return;
            }
        };
    };
}

async fn skip(bot: &Bot, ev: &mumble::event::Message, args: &[String]) {
    let matches = app_for_command("skip")
        .about("Skip the currently playing track")
        .try_get_matches_from(args.iter());
    unwrap_matches!(matches, bot, ev);

    bot.room.next().await;
}

async fn pause(bot: &Bot, ev: &mumble::event::Message, args: &[String]) {
    let matches = app_for_command("pause")
        .about("Pause the currently playing track")
        .try_get_matches_from(args.iter());
    unwrap_matches!(matches, bot, ev);

    bot.room.pause().await;
}

async fn play(bot: &Bot, ev: &mumble::event::Message, args: &[String]) {
    let matches = app_for_command("play")
        .about("Start playing the current track")
        .try_get_matches_from(args.iter());
    unwrap_matches!(matches, bot, ev);

    bot.room.play().await;
}

async fn list(bot: &Bot, ev: &mumble::event::Message, args: &[String]) {
    let matches = app_for_command("list")
        .about("List entries of the current playlist")
        .args(&[
            Arg::new("range")
                .value_name("START:END")
                .about("Range of playlist to output"),
            Arg::new("expand")
                .short('e')
                .long("expand")
                .value_name("DEPTH")
                .about("Expand nested playlists until depth")
                .default_value("1")
                .default_missing_value("99"),
        ])
        .try_get_matches_from(args.iter());
    unwrap_matches!(matches, bot, ev);

    let pl = bot.room.playlist();
    let max_length = bot.client.max_message_length();

    let mut message = String::new();

    if let Some(id) = pl.id() {
        writeln!(message, "{} ({})", pl.title(), id).unwrap();
    } else {
        writeln!(message, "{}", pl.title()).unwrap();
    }

    writeln!(message, "<table><tr><th><u>P</u>os</th><th><u>T</u>itle</th><th><u>A</u>rtist</th><th>A<u>l</u>bum</th></tr>").unwrap();
    writeln!(message, "<tr><th></th><th></th><th>Shuffle</th></tr>").unwrap();

    for (idx, entry) in pl.entries().iter().enumerate() {
        match entry {
            PlaylistLike::Track(tr) => {
                let (artist, album) = ("", ""); // TODO
                writeln!(
                    message,
                    "<tr><td align=\"right\">{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                    idx,
                    tr.title().unwrap_or(""),
                    artist,
                    album
                )
                .unwrap();
            }
            PlaylistLike::Playlist(pl) => {
                writeln!(
                    message,
                    "<tr><td align=\"right\">{}</td><td>{}</td><td>{}</td></tr>",
                    idx,
                    entry.title().unwrap_or(""),
                    if pl.shuffle() { "yes" } else { "no" },
                )
                .unwrap();
            }
        }

        if idx > 3 {
            break;
        }
    }

    writeln!(message, "</table>").unwrap();

    bot.client.respond(&ev, &message).await;
}

async fn new(bot: &Bot, ev: &mumble::event::Message, args: &[String]) {
    let matches = app_for_command("new")
        .about("Create a new playlist")
        .args(&[
            Arg::new("name")
                .value_name("NAME")
                .about("Specify the name of the new playlist"),
            Arg::new("force")
                .short('f')
                .long("force")
                .about("Force replace playlist with unsaved changes"),
        ])
        .try_get_matches_from(args.iter());
    unwrap_matches!(matches, bot, ev);

    let mut playlist = Playlist::new();

    if let Some(name) = matches.value_of("name") {
        playlist.set_title(name);
    }

    bot.room.set_playlist(playlist).await;
}

async fn newsub(bot: &Bot, ev: &mumble::event::Message, args: &[String]) {
    let matches = app_for_command("newsub")
        .about("Attach a new sub-playlist")
        .try_get_matches_from(args.iter());
    unwrap_matches!(matches, bot, ev);

    bot.room.add_playlist(Playlist::new());
}

async fn quit(bot: &Bot, _ev: &mumble::event::Message, _args: &[String]) {
    todo!();
}

// TODO: make this in cmdparser public so I don't have to copy it
/// Tokenize script source, removing comments (starting with `//`).
/// Returns a list of command executions (command + arguments)
fn tokenize(s: &str) -> Vec<Vec<String>> {
    let mut esc = false;
    let mut quoted = false;
    let mut commands = vec![];
    let mut current = vec![];
    let mut sb = String::new();

    fn next_token(sb: &mut String, current: &mut Vec<String>) {
        if !sb.trim().is_empty() {
            current.push((*sb).clone());
        }
        sb.clear();
    }

    fn next_command(sb: &mut String, current: &mut Vec<String>, commands: &mut Vec<Vec<String>>) {
        next_token(sb, current);
        if !current.is_empty() {
            commands.push((*current).clone());
        }
        current.clear();
    }

    for line in s.lines() {
        let get = |i| line.chars().nth(i);

        for (pos, c) in line.chars().enumerate() {
            if esc {
                sb.push(c);
                esc = false;
            } else if !quoted && c == '/' && get(pos + 1) == Some('/') {
                break;
            } else if !quoted && c == ';' {
                next_command(&mut sb, &mut current, &mut commands);
            } else if !quoted && c == ' ' {
                next_token(&mut sb, &mut current);
            } else if c == '"' {
                quoted = !quoted;
            } else if c == '\\' {
                esc = true;
            } else {
                sb.push(c);
            }
        }

        next_command(&mut sb, &mut current, &mut commands);
    }

    commands
}