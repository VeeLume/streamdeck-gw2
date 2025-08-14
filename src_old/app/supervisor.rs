// app/supervisor.rs (reactor-ish)
if !state.in_combat && state.take_any_queued() {
    // build a KeyControl from bindings + selected slots…
    out.push(Command::ExecuteTemplate(key_control));
    out.push(Command::MumbleFastMode(false));
}
