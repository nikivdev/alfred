#!/usr/bin/env swift
// Raise a window using Accessibility API

import Cocoa
import Foundation

guard CommandLine.arguments.count > 1 else {
    print("Usage: raise-window <json-arg>")
    exit(1)
}

let jsonArg = CommandLine.arguments[1]

guard let data = jsonArg.data(using: .utf8),
      let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
      let appName = json["app"] as? String,
      let windowIndex = json["window"] as? Int else {
    print("Invalid argument")
    exit(1)
}

// Find the app by name
let workspace = NSWorkspace.shared
guard let app = workspace.runningApplications.first(where: { $0.localizedName == appName }) else {
    print("App not found: \(appName)")
    exit(1)
}

// Activate the app
app.activate(options: [])

// Wait a tiny bit for activation
usleep(50000) // 50ms

// Get windows via Accessibility API
let axApp = AXUIElementCreateApplication(app.processIdentifier)
var windowsRef: CFTypeRef?
let result = AXUIElementCopyAttributeValue(axApp, kAXWindowsAttribute as CFString, &windowsRef)

guard result == .success,
      let windows = windowsRef as? [AXUIElement],
      windowIndex < windows.count else {
    print("Window not found")
    exit(1)
}

// Raise the window
let window = windows[windowIndex]
AXUIElementPerformAction(window, kAXRaiseAction as CFString)

// Set as main window
AXUIElementSetAttributeValue(window, kAXMainAttribute as CFString, true as CFTypeRef)
