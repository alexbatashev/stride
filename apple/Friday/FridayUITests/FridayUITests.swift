import XCTest

final class FridayUITests: XCTestCase {

    override func setUpWithError() throws {
        continueAfterFailure = false
    }

    @MainActor
    func testChatShellRendersCoreControls() throws {
        let app = XCUIApplication()
        app.launch()

        XCTAssertTrue(app.buttons["newChatButton"].waitForExistence(timeout: 2))
        XCTAssertTrue(app.descendants(matching: .any)["conversationList"].waitForExistence(timeout: 2))
        XCTAssertTrue(app.descendants(matching: .any)["chatInputField"].waitForExistence(timeout: 2))
        XCTAssertTrue(app.buttons["sendMessageButton"].exists)
    }

    @MainActor
    func testLaunchPerformance() throws {
        measure(metrics: [XCTApplicationLaunchMetric()]) {
            XCUIApplication().launch()
        }
    }
}
