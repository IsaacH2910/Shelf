import SwiftUI

struct RootView: View {
    @Bindable var model: AppModel

    var body: some View {
        VStack(spacing: 0) {
            chrome
            content
        }
        .background(ShelfTheme.background.ignoresSafeArea())
        .preferredColorScheme(.dark)
        .sheet(isPresented: $model.showsPicker) {
            LibraryPickerView(model: model)
        }
        .sheet(isPresented: $model.showsSettings) {
            SettingsView(model: model)
        }
        .onAppear { model.start() }
        .onDisappear { model.stop() }
    }

    private var chrome: some View {
        HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                Text("Shelf")
                    .font(.headline)
                    .foregroundStyle(ShelfTheme.text)
                Text(statusText)
                    .font(.caption)
                    .foregroundStyle(statusColor)
            }
            Spacer()
            Button("Find Mac") {
                Task { await model.findMac() }
            }
                .font(.subheadline.weight(.medium))
                .foregroundStyle(ShelfTheme.accent)
            Button("Libraries") { model.showsPicker = true }
                .font(.subheadline.weight(.medium))
                .foregroundStyle(ShelfTheme.accent)
            Button("Settings") { model.showsSettings = true }
                .font(.subheadline.weight(.medium))
                .foregroundStyle(ShelfTheme.accent)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .background(ShelfTheme.surface)
    }

    @ViewBuilder
    private var content: some View {
        if case let .connected(url, _) = model.status {
            ReaderWebView(url: url)
                .ignoresSafeArea(edges: .bottom)
        } else {
            VStack(spacing: 16) {
                ProgressView()
                    .tint(ShelfTheme.accent)
                    .opacity(isBusy ? 1 : 0)
                Text(statusText)
                    .multilineTextAlignment(.center)
                    .foregroundStyle(ShelfTheme.text)
                if let error = model.lastError {
                    Text(error)
                        .font(.footnote)
                        .multilineTextAlignment(.center)
                        .foregroundStyle(ShelfTheme.muted)
                }
                HStack(spacing: 12) {
                    Button("Find Mac") {
                        Task { await model.findMac() }
                    }
                    .buttonStyle(.borderedProminent)
                    .tint(ShelfTheme.accent)
                    Button("Libraries") { model.showsPicker = true }
                        .buttonStyle(.bordered)
                }
            }
            .padding(24)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    private var isBusy: Bool {
        switch model.status {
        case .searching, .connecting:
            return true
        default:
            return false
        }
    }

    private var statusText: String {
        switch model.status {
        case .searching:
            return "1. Bonjour — looking for a Mac in Connect iPhone / iPad mode…"
        case .connecting(let name):
            return "Connecting to \(name)…"
        case .connected(_, .bonjour):
            return "Bonjour"
        case .connected(_, .lan):
            return "LAN"
        case .connected(_, .cloud):
            return "Cloud"
        case .offline(let message):
            return message
        case .connected(_, .offline):
            return "Offline"
        }
    }

    private var statusColor: Color {
        switch model.status.kind {
        case .offline:
            return .red.opacity(0.85)
        case .bonjour, .lan, .cloud:
            return ShelfTheme.accent
        }
    }
}
