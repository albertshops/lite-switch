import SwiftUI

struct SettingsView: View {
    @ObservedObject var model: SettingsModel
    @State private var confirmsHistoryReset = false

    var body: some View {
        Form {
            LabeledContent("Keyboard Shortcut") {
                ShortcutRecorder(
                    shortcut: model.shortcut,
                    onRecord: model.setShortcut,
                    onInvalid: model.rejectShortcutWithoutModifiers
                )
                .frame(width: 150, height: 30)
                .accessibilityLabel("Global keyboard shortcut")
            }

            if let error = model.shortcutError {
                Label(error, systemImage: "exclamationmark.triangle.fill")
                    .font(.caption)
                    .foregroundStyle(.red)
                    .accessibilityLabel("Shortcut error: \(error)")
            }

            Toggle(
                "Launch at Login",
                isOn: Binding(
                    get: { model.launchAtLoginEnabled },
                    set: model.setLaunchAtLogin
                )
            )
            .accessibilityLabel("Launch Lite Switch at Login")

            if let message = model.launchAtLoginMessage {
                Label(message, systemImage: "info.circle")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .accessibilityLabel("Launch at Login status: \(message)")
            }

            Divider()

            Button("Reset Usage History…", role: .destructive) {
                confirmsHistoryReset = true
            }
            .accessibilityLabel("Reset application usage history")

            if let message = model.usageHistoryMessage {
                Label(message, systemImage: "checkmark.circle")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .accessibilityLabel(message)
            }
        }
        .formStyle(.grouped)
        .padding()
        .frame(width: 460, height: 300)
        .confirmationDialog(
            "Reset Usage History?",
            isPresented: $confirmsHistoryReset,
            titleVisibility: .visible
        ) {
            Button("Reset Usage History", role: .destructive) {
                model.resetUsageHistory()
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Lite Switch will forget application launch counts and recency. Your shortcut and Launch at Login setting will not change.")
        }
    }
}
