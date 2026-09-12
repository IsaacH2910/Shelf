import SwiftUI

@main
struct ShelfApp: App {
    @State private var model = AppModel()

    var body: some Scene {
        WindowGroup {
            RootView(model: model)
                .background(ShelfTheme.background.ignoresSafeArea())
        }
    }
}
