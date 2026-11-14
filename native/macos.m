// Thin AppKit / Accessibility / IOKit adapter. Device protocol and lifecycle live in Rust.
#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>
#import <IOKit/IOKitLib.h>
#include <unistd.h>

static void (*selection)(int);
static NSStatusItem *statusItem;
static NSMenuItem *statusLine;
static NSArray<NSMenuItem *> *choices;
static BOOL busy;
static NSString *statusText;
static NSWindow *setupWindow;
static NSString *profileName(int mode) { return mode == 1 ? @"Dubplate GRV6" : @"Dubplate XZ"; }
static NSString *serial(int mode) { return mode == 1 ? @"DUBPLATE-GRV6-0001" : @"DUBPLATE-XZ-0001"; }
static int fail(char *out, size_t n, NSString *message) {
    snprintf(out, n, "%s", message.UTF8String); return 0;
}
static id attr(AXUIElementRef el, CFStringRef key) {
    CFTypeRef value = NULL;
    if (AXUIElementCopyAttributeValue(el, key, &value) != kAXErrorSuccess) return nil;
    return CFBridgingRelease(value);
}
static AXUIElementRef find(AXUIElementRef root, NSString *key, NSString *value, BOOL prefix, unsigned depth) {
    if (!root || depth > 18) return NULL;
    id v = attr(root, (__bridge CFStringRef)key);
    if ([v isKindOfClass:NSString.class] && (prefix ? [v hasPrefix:value] : [v isEqualToString:value])) {
        return (AXUIElementRef)CFRetain(root);
    }
    NSArray *children = attr(root, kAXChildrenAttribute);
    for (id child in children) {
        AXUIElementRef found = find((__bridge AXUIElementRef)child, key, value, prefix, depth + 1);
        if (found) return found;
    }
    return NULL;
}
static AXUIElementRef byID(AXUIElementRef root, NSString *identifier) {
    return find(root, @"AXIdentifier", identifier, NO, 0);
}
static BOOL containsText(AXUIElementRef root, NSString *text, unsigned depth) {
    if (!root || depth > 18) return NO;
    for (NSString *key in @[@"AXValue", @"AXTitle", @"AXDescription"]) {
        id value = attr(root, (__bridge CFStringRef)key);
        if ([value isKindOfClass:NSString.class] && [value containsString:text]) return YES;
    }
    for (id child in attr(root, kAXChildrenAttribute)) {
        if (containsText((__bridge AXUIElementRef)child, text, depth + 1)) return YES;
    }
    return NO;
}
static BOOL press(AXUIElementRef el) {
    if (!el) return NO;
    BOOL ok = AXUIElementPerformAction(el, kAXPressAction) == kAXErrorSuccess;
    CFRelease(el); return ok;
}
static BOOL has(AXUIElementRef root, NSString *identifier) {
    AXUIElementRef el = byID(root, identifier);
    if (!el) return NO; CFRelease(el); return YES;
}
static BOOL waitID(AXUIElementRef root, NSString *identifier, double seconds) {
    for (int i=0; i < seconds*10; ++i) {
        if (has(root, identifier)) return YES;
        usleep(100000);
    }
    return NO;
}
static BOOL detachCommand(AXUIElementRef app) {
    // Jig's SwiftUI Detach button acknowledges AXPress without reliably acting.
    // Its standard Session > Detach menu command has a native action target.
    id bar = attr(app, kAXMenuBarAttribute);
    if (!bar) return NO;
    AXUIElementRef root = (__bridge AXUIElementRef)bar;
    AXUIElementRef item = find(root, @"AXTitle", @"Detach", NO, 0);
    if (item) return press(item);
    if (!press(find(root, @"AXTitle", @"Session", NO, 0))) return NO;
    for (int i=0; i<20; ++i) {
        item = find(root, @"AXTitle", @"Detach", NO, 0);
        if (item) return press(item);
        usleep(100000);
    }
    return NO;
}

int db_usb_present(int mode) {
    @autoreleasepool {
        NSMutableDictionary *match = CFBridgingRelease(IOServiceMatching("IOUSBHostDevice"));
        match[@"idVendor"] = @0x2b73;
        match[@"idProduct"] = mode == 1 ? @0x004c : @0x002c;
        io_iterator_t iterator = 0;
        if (IOServiceGetMatchingServices(kIOMainPortDefault, (__bridge_retained CFDictionaryRef)match, &iterator)) return -1;
        io_service_t device; int found = 0;
        while ((device = IOIteratorNext(iterator))) {
            NSString *s = CFBridgingRelease(IORegistryEntryCreateCFProperty(device, CFSTR("USB Serial Number"), kCFAllocatorDefault, 0));
            if ([s isEqualToString:serial(mode)]) found++;
            IOObjectRelease(device);
        }
        IOObjectRelease(iterator); return found;
    }
}
int db_usb_conflict(void) {
    @autoreleasepool {
        for (NSNumber *pid in @[@0x002c, @0x002d, @0x004c, @0x0041]) {
            NSMutableDictionary *match = CFBridgingRelease(IOServiceMatching("IOUSBHostDevice"));
            match[@"idVendor"] = @0x2b73; match[@"idProduct"] = pid;
            io_iterator_t it = 0;
            if (IOServiceGetMatchingServices(kIOMainPortDefault, (__bridge_retained CFDictionaryRef)match, &it)) return 1;
            io_service_t dev; BOOL conflict = NO;
            while ((dev = IOIteratorNext(it))) {
                NSString *s = CFBridgingRelease(IORegistryEntryCreateCFProperty(dev, CFSTR("USB Serial Number"), kCFAllocatorDefault, 0));
                if (![s isEqualToString:serial(1)] && ![s isEqualToString:serial(2)]) conflict = YES;
                IOObjectRelease(dev);
            }
            IOObjectRelease(it); if (conflict) return 1;
        }
        return 0;
    }
}
int db_accessibility(void) { return AXIsProcessTrusted(); }
static NSURL *jigURL(void) {
    NSURL *url = [NSWorkspace.sharedWorkspace URLForApplicationWithBundleIdentifier:@"fr.neimad.jig"];
    if (url && [NSFileManager.defaultManager fileExistsAtPath:url.path]) return url;
    for (NSString *p in @[@"/Applications/Jig.app", @"/Volumes/Jig 0.1.0/Jig.app"]) {
        if ([NSFileManager.defaultManager fileExistsAtPath:p]) return [NSURL fileURLWithPath:p];
    }
    return nil;
}
static NSRunningApplication *jigApp(void) {
    NSArray *apps = [NSRunningApplication runningApplicationsWithBundleIdentifier:@"fr.neimad.jig"];
    return apps.count == 1 ? apps.firstObject : nil;
}
static BOOL openJig(NSURL *file) {
    NSURL *url = jigURL(); if (!url) return NO;
    dispatch_async(dispatch_get_main_queue(), ^{
        NSWorkspaceOpenConfiguration *cfg = [NSWorkspaceOpenConfiguration configuration];
        cfg.activates = NO;
        if (file) {
            [NSWorkspace.sharedWorkspace openURLs:@[file] withApplicationAtURL:url configuration:cfg completionHandler:nil];
        } else {
            [NSWorkspace.sharedWorkspace openApplicationAtURL:url configuration:cfg completionHandler:nil];
        }
    });
    return YES;
}
static BOOL choose(AXUIElementRef app, int mode) {
    if (!press(byID(app, @"tab.library")) || !waitID(app, @"library.search", 3)) return NO;
    AXUIElementRef search = byID(app, @"library.search");
    AXError result = AXUIElementSetAttributeValue(search, kAXValueAttribute, (__bridge CFStringRef)profileName(mode));
    CFRelease(search); if (result != kAXErrorSuccess) return NO;
    for (int i=0; i<30; ++i) {
        AXUIElementRef button = find(app, @"AXDescription", [profileName(mode) stringByAppendingString:@","], YES, 0);
        if (button) {
            if (!press(button)) return NO;
            for (int j=0; j<30; ++j) {
                AXUIElementRef name = byID(app, @"field.name");
                BOOL right = name && [attr(name, kAXValueAttribute) isEqual:profileName(mode)];
                if (name) CFRelease(name);
                if (right) return YES;
                usleep(100000);
            }
            return NO;
        }
        usleep(100000);
    }
    return NO;
}
// Called on Rust's worker thread; never blocks AppKit's main loop.
int db_jig_switch(int mode, const char *profile, int attach, char *error, size_t capacity) {
    @autoreleasepool {
        if (!AXIsProcessTrusted()) return fail(error, capacity, @"Allow Dubplate Devices in System Settings → Privacy & Security → Accessibility, then retry.");
        if ([NSRunningApplication runningApplicationsWithBundleIdentifier:@"fr.neimad.jig"].count > 1)
            return fail(error, capacity, @"Keep one Jig instance open; close the extra Jig instances and retry.");
        if (!jigApp()) {
            if (!attach) return db_usb_present(mode) == 0 ? 1 : fail(error, capacity, @"USB device remains, but Jig is not running.");
            if (!openJig(nil)) return fail(error, capacity, @"Install Jig from neimad.fr/apps/jig, then retry.");
            for (int i=0; i<100 && !jigApp(); ++i) usleep(100000);
        }
        NSRunningApplication *running = jigApp();
        if (!running) return fail(error, capacity, @"Jig did not open. Launch Jig once, then retry.");
        // SwiftUI can leave a hidden window's accessibility tree stale.
        // Reveal it behind the foreground app while operating its controls.
        dispatch_sync(dispatch_get_main_queue(), ^{ [running unhide]; });
        AXUIElementRef app = AXUIElementCreateApplication(running.processIdentifier);
        AXUIElementSetMessagingTimeout(app, 1.0);
        BOOL selected = choose(app, mode);
        if (!selected && attach) {
            if (!openJig([NSURL fileURLWithPath:@(profile)])) { CFRelease(app); return fail(error, capacity, @"Could not open the device profile in Jig."); }
            if (waitID(app, @"import.confirm", 6)) {
                if (containsText(app, profileName(mode), 0)) {
                    press(byID(app, @"import.confirm"));
                    for (int i=0; i<50 && has(app, @"import.confirm"); ++i) usleep(100000);
                }
            }
            selected = choose(app, mode);
        }
        if (!selected) { CFRelease(app); return fail(error, capacity, @"Could not select the device profile. Open Jig, import the included profile, then retry."); }
        AXUIElementRef serialField = byID(app, @"field.iserialnumber");
        BOOL owned = serialField && [attr(serialField, kAXValueAttribute) isEqual:serial(mode)];
        if (serialField) CFRelease(serialField);
        if (!owned) { CFRelease(app); return fail(error, capacity, @"The selected Jig profile has changed. Reimport the included profile before retrying."); }
        // Never replace or detach a different user's device.
        if (has(app, @"workbench.mount.other")) { CFRelease(app); return fail(error, capacity, @"Jig has another device attached. Detach it in Jig, then retry."); }
        NSString *action = attach ? @"workbench.mount.idle" : @"workbench.mount.mounted";
        int current = db_usb_present(mode);
        if (current < 0 || (attach ? current != 1 : current != 0)) {
            if (!waitID(app, action, 3) || !(attach ? press(byID(app, action)) : detachCommand(app))) {
                CFRelease(app); return fail(error, capacity, @"Jig rejected the attach/detach action. Open Jig and retry.");
            }
        }
        BOOL confirmed = NO;
        for (int i=0; i<80; ++i) {
            int present = db_usb_present(mode);
            if (present >= 0 && (attach ? present == 1 : present == 0)) { confirmed = YES; break; }
            usleep(100000);
        }
        CFRelease(app);
        if (!confirmed) return fail(error, capacity, @"USB attachment did not reach the requested state. Open Jig to inspect it.");
        // Only hide the background app after a confirmed operation.
        dispatch_async(dispatch_get_main_queue(), ^{ [running hide]; });
        return 1;
    }
}

@interface DeviceMenu : NSObject <NSApplicationDelegate, NSMenuDelegate>
@end
@implementation DeviceMenu
- (void)pick:(NSMenuItem *)sender { if (!busy || sender.tag == 0) selection((int)sender.tag); }
- (void)quit:(id)sender { selection(9); }
- (void)showJig:(id)sender { NSRunningApplication *app = jigApp(); if (app) [app activateWithOptions:NSApplicationActivateAllWindows]; else openJig(nil); }
- (void)setup:(id)sender {
    NSDictionary *options = @{(__bridge NSString *)kAXTrustedCheckOptionPrompt: @YES};
    AXIsProcessTrustedWithOptions((__bridge CFDictionaryRef)options);
    [NSWorkspace.sharedWorkspace openURL:[NSURL URLWithString:@"x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"]];
}
- (void)profiles:(id)sender {
    [NSWorkspace.sharedWorkspace openURL:NSBundle.mainBundle.resourceURL];
}
- (void)details:(id)sender {
    NSAlert *alert = [NSAlert new]; alert.messageText = @"Dubplate Devices";
    alert.informativeText = statusText ?: @"Off"; [alert runModal];
}
- (void)selector:(id)sender {
    selection(8);
    // The setup button also works when a crowded menu bar hides the status icon.
    [statusItem.menu popUpMenuPositioningItem:nil atLocation:NSMakePoint(0, 0) inView:sender];
}
- (BOOL)applicationShouldHandleReopen:(NSApplication *)app hasVisibleWindows:(BOOL)visible {
    if (setupWindow) [setupWindow makeKeyAndOrderFront:nil];
    else [statusItem.button performClick:nil];
    return NO;
}
- (NSApplicationTerminateReply)applicationShouldTerminate:(NSApplication *)sender { selection(9); return NSTerminateCancel; }
@end
static DeviceMenu *delegate;
void db_ui_update(int mode, const char *text, int working) {
    NSString *label = @(text);
    dispatch_async(dispatch_get_main_queue(), ^{
        busy = working;
        statusText = label;
        statusLine.title = label.length > 68 ? [[label substringToIndex:65] stringByAppendingString:@"…"] : label;
        statusLine.toolTip = label;
        statusItem.button.title = mode == 1 ? @" GRV6" : mode == 2 ? @" XZ" : @"";
        statusItem.button.toolTip = [@"Dubplate Devices — " stringByAppendingString:label];
        for (NSMenuItem *item in choices) {
            item.state = item.tag == mode ? NSControlStateValueOn : NSControlStateValueOff;
            item.enabled = !busy || item.tag == 0;
        }
    });
}
void db_ui_exit(void) { dispatch_async(dispatch_get_main_queue(), ^{
    [NSApp stop:nil];
    [NSApp postEvent:[NSEvent otherEventWithType:NSEventTypeApplicationDefined location:NSZeroPoint modifierFlags:0 timestamp:0 windowNumber:0 context:nil subtype:0 data1:0 data2:0] atStart:NO];
}); }
void db_ui_run(void (*callback)(int)) {
    @autoreleasepool {
        selection = callback;
        [NSApplication sharedApplication];
        [NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory];
        delegate = [DeviceMenu new]; NSApp.delegate = delegate;
        [NSApp finishLaunching];
        statusItem = [NSStatusBar.systemStatusBar statusItemWithLength:NSVariableStatusItemLength];
        NSImage *icon = [NSImage imageWithSystemSymbolName:@"opticaldisc" accessibilityDescription:@"Dubplate Devices"];
        icon.template = YES; statusItem.button.image = icon;
        statusItem.button.imagePosition = NSImageLeft;
        NSMenu *menu = [[NSMenu alloc] initWithTitle:@"Dubplate Devices"]; menu.autoenablesItems = NO;
        NSMenuItem *title = [[NSMenuItem alloc] initWithTitle:@"Dubplate Devices" action:NULL keyEquivalent:@""];
        title.enabled = NO; [menu addItem:title];
        statusLine = [[NSMenuItem alloc] initWithTitle:@"Starting…" action:@selector(details:) keyEquivalent:@""]; statusLine.target = delegate; [menu addItem:statusLine];
        [menu addItem:NSMenuItem.separatorItem];
        NSMutableArray *items = [NSMutableArray array];
        NSArray *names = @[@"Off", @"DDJ-GRV6", @"XDJ-XZ"];
        for (int i=0; i<3; ++i) {
            NSMenuItem *item = [[NSMenuItem alloc] initWithTitle:names[i] action:@selector(pick:) keyEquivalent:@""];
            item.target = delegate; item.tag = i; [items addObject:item]; [menu addItem:item];
        }
        choices = items;
        [menu addItem:NSMenuItem.separatorItem];
        for (NSArray *entry in @[@[@"Open Jig", @"showJig:"], @[@"Enable control…", @"setup:"], @[@"Device profiles…", @"profiles:"], @[@"Quit", @"quit:"]]) {
            NSMenuItem *item = [[NSMenuItem alloc] initWithTitle:entry[0] action:NSSelectorFromString(entry[1]) keyEquivalent:@""];
            item.target = delegate; [menu addItem:item];
        }
        statusItem.menu = menu;
        if (!AXIsProcessTrusted()) {
            setupWindow = [[NSWindow alloc] initWithContentRect:NSMakeRect(0, 0, 400, 210)
                styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable backing:NSBackingStoreBuffered defer:NO];
            setupWindow.title = @"Dubplate Devices"; setupWindow.releasedWhenClosed = NO;
            NSTextField *heading = [NSTextField labelWithString:@"Your devices, in the menu bar."];
            heading.font = [NSFont boldSystemFontOfSize:19]; heading.frame = NSMakeRect(24, 153, 355, 30);
            [setupWindow.contentView addSubview:heading];
            NSTextField *body = [NSTextField wrappingLabelWithString:@"Choose Off, DDJ-GRV6 or XDJ-XZ from the record icon. Allow Accessibility control so this app can switch your installed Jig in the background."];
            body.frame = NSMakeRect(24, 65, 350, 76); [setupWindow.contentView addSubview:body];
            NSButton *enable = [NSButton buttonWithTitle:@"Enable control…" target:delegate action:@selector(setup:)];
            enable.frame = NSMakeRect(20, 20, 165, 32); [setupWindow.contentView addSubview:enable];
            NSButton *show = [NSButton buttonWithTitle:@"Open selector" target:delegate action:@selector(selector:)];
            show.frame = NSMakeRect(215, 20, 165, 32); [setupWindow.contentView addSubview:show];
            [setupWindow center]; [setupWindow makeKeyAndOrderFront:nil];
            [NSApp activateIgnoringOtherApps:YES];
        }
        [NSApp run];
    }
}
