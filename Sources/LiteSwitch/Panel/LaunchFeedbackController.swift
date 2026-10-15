import AppKit

/// Brief launch acknowledgement that never takes focus or intercepts clicks.
@MainActor
final class LaunchFeedbackController {
    private var panel: NSPanel?
    private var dismissalTask: Task<Void, Never>?
    private var presentationID = UUID()

    func show(appName: String) {
        dismissalTask?.cancel()
        let id = UUID()
        presentationID = id

        let panel = self.panel ?? makePanel()
        self.panel = panel
        let label = NSTextField(labelWithString: "Opening \(appName)…")
        label.font = .systemFont(ofSize: 14, weight: .medium)
        label.textColor = .labelColor
        label.lineBreakMode = .byTruncatingTail
        label.alignment = .center
        label.translatesAutoresizingMaskIntoConstraints = false

        let background = NSVisualEffectView()
        background.material = .hudWindow
        background.blendingMode = .behindWindow
        background.state = .active
        background.wantsLayer = true
        background.layer?.cornerRadius = 12
        background.layer?.masksToBounds = true
        background.addSubview(label)
        NSLayoutConstraint.activate([
            label.leadingAnchor.constraint(equalTo: background.leadingAnchor, constant: 20),
            label.trailingAnchor.constraint(equalTo: background.trailingAnchor, constant: -20),
            label.centerYAnchor.constraint(equalTo: background.centerYAnchor),
        ])
        panel.contentView = background

        let screen = NSScreen.screens.first { NSMouseInRect(NSEvent.mouseLocation, $0.frame, false) }
            ?? NSScreen.main
        guard let frame = screen?.visibleFrame else { return }
        let width = min(max(label.intrinsicContentSize.width + 40, 180), min(480, frame.width - 32))
        panel.setFrame(NSRect(x: frame.midX - width / 2, y: frame.maxY - 64, width: width, height: 44), display: true)
        panel.alphaValue = 1
        panel.orderFrontRegardless()

        dismissalTask = Task { [weak self] in
            do {
                try await Task.sleep(for: .seconds(1.1))
            } catch {
                return
            }
            guard let self, self.presentationID == id else { return }
            NSAnimationContext.runAnimationGroup { context in
                context.duration = 0.3
                panel.animator().alphaValue = 0
            } completionHandler: { [weak self] in
                Task { @MainActor in
                    guard self?.presentationID == id else { return }
                    panel.orderOut(nil)
                }
            }
        }
    }

    private func makePanel() -> NSPanel {
        let panel = NSPanel(
            contentRect: .zero,
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.level = .statusBar
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .ignoresCycle]
        panel.ignoresMouseEvents = true
        panel.hidesOnDeactivate = false
        panel.isReleasedWhenClosed = false
        return panel
    }
}
