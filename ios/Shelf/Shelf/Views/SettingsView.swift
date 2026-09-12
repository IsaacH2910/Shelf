import SwiftUI

struct SettingsView: View {
    @Bindable var model: AppModel
    @State private var hostname: String = ""
    @State private var lanURL: String = ""

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Text("1. Bonjour — this app finds the Mac when Connect iPhone / iPad is on.\n2. LAN URL — same Wi-Fi if Bonjour is not available.\n3. Cloudflare — HTTPS when local fails.")
                        .foregroundStyle(ShelfTheme.muted)
                } header: {
                    Text("How this app connects")
                }

                Section {
                    TextField("http://studio.local:7834", text: $lanURL)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .keyboardType(.URL)
                    Button("Save LAN URL") {
                        model.saveManualLanURL(lanURL)
                    }
                    .foregroundStyle(ShelfTheme.accent)
                } header: {
                    Text("2. LAN URL")
                } footer: {
                    Text("Used when Bonjour does not find the Mac. Same address the Mac shows under Connect iPhone / iPad. Windows, Android, and Linux clients use this path instead of Bonjour.")
                }

                Section {
                    TextField("shelf.example.com", text: $hostname)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .keyboardType(.URL)
                    Button("Save hostname") {
                        model.saveCloudHostname(hostname)
                    }
                    .foregroundStyle(ShelfTheme.accent)
                } header: {
                    Text("3. Cloudflare")
                } footer: {
                    Text("Used last, when this phone is away or nearby mode is off. Sign-in cookies stay on that HTTPS origin.")
                }

                Section("On the Mac") {
                    Text("Open Shelf → Settings → Connect iPhone / iPad. Then open this app and tap Find Mac. A sleeping Mac cannot advertise.")
                }

                Section("Security") {
                    Text("LAN is HTTP on your Wi-Fi and still needs a household password. Cloudflare stays HTTPS for remote. Switching origins may ask you to sign in again.")
                }
            }
            .scrollContentBackground(.hidden)
            .background(ShelfTheme.background)
            .navigationTitle("Settings")
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { model.showsSettings = false }
                        .foregroundStyle(ShelfTheme.accent)
                }
            }
            .onAppear {
                hostname = model.cloudHostname
                lanURL = model.manualLanText
            }
        }
        .preferredColorScheme(.dark)
    }
}
