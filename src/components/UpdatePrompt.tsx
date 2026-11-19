// Auto-update is off for this fork (1.0.7-2): the updater plugin was
// removed from main.rs because registering it without a `pubkey` in
// tauri.conf.json panicked on startup. This component renders nothing
// so no code ever calls the removed plugin. New versions come from
// GitHub Releases. Kept as a stub so App.tsx needs no change.
export default function UpdatePrompt() {
  return null;
}
