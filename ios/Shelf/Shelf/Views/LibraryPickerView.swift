import SwiftUI

struct LibraryPickerView: View {
    @Bindable var model: AppModel

    var body: some View {
        NavigationStack {
            List {
                Section {
                    if model.instances.isEmpty {
                        Text("No Mac is advertising yet. On the Mac: Settings → Connect iPhone / iPad, then tap Find Mac here.")
                            .foregroundStyle(ShelfTheme.muted)
                    }
                    ForEach(model.instances) { instance in
                        Button {
                            Task { await model.connect(to: instance) }
                        } label: {
                            VStack(alignment: .leading, spacing: 4) {
                                Text(instance.displayName)
                                    .foregroundStyle(ShelfTheme.text)
                                if let url = instance.advertisedURL {
                                    Text(url.absoluteString)
                                        .font(.caption)
                                        .foregroundStyle(ShelfTheme.muted)
                                } else {
                                    Text("Bonjour · tap to resolve")
                                        .font(.caption)
                                        .foregroundStyle(ShelfTheme.muted)
                                }
                            }
                        }
                    }
                } header: {
                    Text("1. Bonjour")
                }

                Section {
                    if let last = Preferences().lastLocalURL {
                        Button("Use \(last.absoluteString)") {
                            Task { await model.connect(to: last, kind: .lan) }
                        }
                    } else if !model.manualLanText.isEmpty, let url = Preferences.parseURL(model.manualLanText) {
                        Button("Use \(url.absoluteString)") {
                            Task { await model.connect(to: url, kind: .lan) }
                        }
                    } else {
                        Text("For Windows, Android, or Linux browsers — or if Bonjour misses — type the LAN URL from the Mac (http://hostname.local:7834) in Settings.")
                            .foregroundStyle(ShelfTheme.muted)
                    }
                } header: {
                    Text("2. LAN URL")
                }

                Section {
                    if let url = URL(string: "https://\(model.cloudHostname)"), !model.cloudHostname.isEmpty {
                        Button("Use \(url.host ?? model.cloudHostname)") {
                            Task { await model.useCloud() }
                        }
                    } else {
                        Text("Last resort when you are away. Set the public hostname in Settings.")
                            .foregroundStyle(ShelfTheme.muted)
                    }
                } header: {
                    Text("3. Cloudflare")
                }
            }
            .scrollContentBackground(.hidden)
            .background(ShelfTheme.background)
            .navigationTitle("Libraries")
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Find Mac") {
                        Task { await model.findMac() }
                    }
                    .foregroundStyle(ShelfTheme.accent)
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Close") { model.showsPicker = false }
                        .foregroundStyle(ShelfTheme.accent)
                }
            }
        }
        .preferredColorScheme(.dark)
    }
}
