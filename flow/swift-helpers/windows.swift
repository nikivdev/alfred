#!/usr/bin/env swift
// Fast window enumeration using Accessibility API
// Gets windows of the app that was frontmost before Alfred

import Cocoa
import Foundation

struct AlfredItem: Codable {
    let title: String
    let subtitle: String
    let arg: String
    let match: String
    let icon: AlfredIcon
    var valid: Bool?
}

struct AlfredIcon: Codable {
    let type: String
    let path: String
}

struct AlfredOutput: Codable {
    let items: [AlfredItem]
    var rerun: Double?
}

// Get the app that owns the menu bar - this is the app that was active before Alfred
// Alfred doesn't take over the menu bar, so this gives us the previous app
let workspace = NSWorkspace.shared

var targetApp: NSRunningApplication? = workspace.menuBarOwningApplication

// If menu bar app is nil or is Alfred, fall back to finding first non-Alfred app with windows
if targetApp == nil || targetApp?.bundleIdentifier == "com.runningwithcrayons.Alfred" {
    for app in workspace.runningApplications {
        guard app.activationPolicy == .regular,
              app.bundleIdentifier != "com.runningwithcrayons.Alfred",
              !app.isTerminated else { continue }

        // Check if this app has windows via Accessibility
        let axApp = AXUIElementCreateApplication(app.processIdentifier)
        var windowsRef: CFTypeRef?
        let result = AXUIElementCopyAttributeValue(axApp, kAXWindowsAttribute as CFString, &windowsRef)

        if result == .success, let windows = windowsRef as? [AXUIElement], !windows.isEmpty {
            for window in windows {
                var titleRef: CFTypeRef?
                AXUIElementCopyAttributeValue(window, kAXTitleAttribute as CFString, &titleRef)
                if let title = titleRef as? String, !title.isEmpty {
                    targetApp = app
                    break
                }
            }
            if targetApp != nil && targetApp?.bundleIdentifier != "com.runningwithcrayons.Alfred" {
                break
            }
        }
    }
}

guard let app = targetApp, app.bundleIdentifier != "com.runningwithcrayons.Alfred" else {
    let output = AlfredOutput(items: [AlfredItem(
        title: "No app found",
        subtitle: "Could not determine frontmost app",
        arg: "",
        match: "",
        icon: AlfredIcon(type: "fileicon", path: "/System/Applications/Finder.app"),
        valid: false
    )])
    print(String(data: try! JSONEncoder().encode(output), encoding: .utf8)!)
    exit(0)
}

let appName = app.localizedName ?? "Unknown"
let appPath = app.bundleURL?.path ?? "/System/Applications/Finder.app"
let appPID = app.processIdentifier

// Use Accessibility API to get windows
let axApp = AXUIElementCreateApplication(appPID)
var windowsRef: CFTypeRef?
let result = AXUIElementCopyAttributeValue(axApp, kAXWindowsAttribute as CFString, &windowsRef)

var items: [AlfredItem] = []

if result == .success, let windows = windowsRef as? [AXUIElement] {
    for (index, window) in windows.enumerated() {
        var titleRef: CFTypeRef?
        AXUIElementCopyAttributeValue(window, kAXTitleAttribute as CFString, &titleRef)

        guard let title = titleRef as? String, !title.isEmpty else {
            continue
        }

        let argData: [String: Any] = [
            "app": appName,
            "window": index,
            "title": title,
            "pid": appPID
        ]

        let argJSON = try! JSONSerialization.data(withJSONObject: argData)
        let argString = String(data: argJSON, encoding: .utf8)!

        items.append(AlfredItem(
            title: title,
            subtitle: appName,
            arg: argString,
            match: title,
            icon: AlfredIcon(type: "fileicon", path: appPath)
        ))
    }
}

if items.isEmpty {
    items.append(AlfredItem(
        title: "No windows found",
        subtitle: appName,
        arg: "",
        match: "",
        icon: AlfredIcon(type: "fileicon", path: appPath),
        valid: false
    ))
}

let output = AlfredOutput(items: items)
let encoder = JSONEncoder()
print(String(data: try! encoder.encode(output), encoding: .utf8)!)
