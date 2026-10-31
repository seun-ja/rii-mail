#import <AppKit/AppKit.h>

// The Tauri plugin sends desktop notifications without exposing their activation.
// Keep delivery and delegate callbacks asynchronous on the application's main loop.
typedef void (*RiiMailNotificationCallback)(const char *, bool);
static RiiMailNotificationCallback notificationCallback;

@interface RiiMailNotificationDelegate : NSObject <NSUserNotificationCenterDelegate>
@end

@implementation RiiMailNotificationDelegate
- (BOOL)userNotificationCenter:(NSUserNotificationCenter *)center
    shouldPresentNotification:(NSUserNotification *)notification {
    return YES;
}

- (void)userNotificationCenter:(NSUserNotificationCenter *)center
    didActivateNotification:(NSUserNotification *)notification {
    BOOL clicked = notification.activationType == NSUserNotificationActivationTypeContentsClicked ||
                   notification.activationType == NSUserNotificationActivationTypeActionButtonClicked;
    if (notificationCallback && notification.identifier) {
        notificationCallback(notification.identifier.UTF8String, clicked);
    }
    [center removeDeliveredNotification:notification];
}

@end

// Called on the main thread. Strings are copied before returning to Rust.
void riimail_show_notification(const char *identifier, const char *subject,
                               RiiMailNotificationCallback callback) {
    @autoreleasepool {
        static RiiMailNotificationDelegate *delegate;
        static dispatch_once_t once;
        dispatch_once(&once, ^{
            delegate = [RiiMailNotificationDelegate new];
        });
        notificationCallback = callback;
        NSUserNotificationCenter *center = NSUserNotificationCenter.defaultUserNotificationCenter;
        center.delegate = delegate;
        NSUserNotification *notification = [NSUserNotification new];
        notification.identifier = [NSString stringWithUTF8String:identifier];
        notification.title = @"New Mail";
        notification.hasActionButton = YES;
        notification.actionButtonTitle = @"Open";
        notification.informativeText = [NSString stringWithUTF8String:subject];
        [center deliverNotification:notification];
    }
}
