#import <AppKit/AppKit.h>
#import <Sparkle/Sparkle.h>

@interface SFUpdaterDelegate : NSObject <SPUUpdaterDelegate>
@property (copy) NSString *pendingVersion;
#ifdef SF_UPDATER_TESTING
@property NSError *lastError;
#endif
@end

@implementation SFUpdaterDelegate
- (BOOL)updater:(__unused SPUUpdater *)updater willInstallUpdateOnQuit:(SUAppcastItem *)item
    immediateInstallationBlock:(__unused void (^)(void))install {
    self.pendingVersion = item.displayVersionString;
    // Sparkle still installs on termination, without interrupting a viewing session.
    return YES;
}
- (void)updater:(__unused SPUUpdater *)updater didAbortWithError:(NSError *)error {
#ifdef SF_UPDATER_TESTING
    self.lastError = error;
#endif
    NSLog(@"Petalook update check: %@", error.localizedDescription);
}
@end

static SPUStandardUpdaterController *controller;
static SFUpdaterDelegate *delegate;
static NSString *startupError;
static BOOL initialized;

static BOOL validFeed(NSString *feed) {
    NSURL *url = [NSURL URLWithString:feed];
#ifdef SF_UPDATER_TESTING
    if ([url.scheme isEqualToString:@"http"] && [url.host isEqualToString:@"127.0.0.1"]) return YES;
#endif
    return [url.scheme isEqualToString:@"https"] && url.host.length > 0 && !url.user && !url.password;
}

static BOOL initializeUpdater(void) {
    if (initialized) return controller != nil;
    initialized = YES;
    NSDictionary *info = NSBundle.mainBundle.infoDictionary;
    NSString *feed = info[@"SUFeedURL"];
    NSString *publicKey = info[@"SUPublicEDKey"];
    if (!feed.length || !publicKey.length) {
        startupError = @"This build isn’t connected to an update channel yet.";
        return NO;
    }
    if (!validFeed(feed) || [[NSData alloc] initWithBase64EncodedString:publicKey options:0].length != 32) {
        startupError = @"The update channel could not be verified.";
        return NO;
    }
    // Loading after the first draw keeps Sparkle and its helpers off the launch path.
    NSString *path = [NSBundle.mainBundle.privateFrameworksPath stringByAppendingPathComponent:@"Sparkle.framework"];
    NSError *error = nil;
    NSBundle *framework = [NSBundle bundleWithPath:path];
    if (!framework || ![framework loadAndReturnError:&error]) {
        startupError = @"The updater couldn’t be opened. Please reinstall Petalook.";
        NSLog(@"Petalook updater: %@", error);
        return NO;
    }
    Class updaterClass = NSClassFromString(@"SPUStandardUpdaterController");
    delegate = [SFUpdaterDelegate new];
    SPUStandardUpdaterController *candidate = [[updaterClass alloc] initWithStartingUpdater:NO
        updaterDelegate:delegate userDriverDelegate:nil];
    if (!candidate || ![candidate.updater startUpdater:&error]) {
        startupError = @"The updater couldn’t start. Please try again after restarting Petalook.";
        NSLog(@"Petalook updater: %@", error);
        return NO;
    }
    controller = candidate;
    return YES;
}

void sf_updater_start(void) {
    dispatch_async(dispatch_get_main_queue(), ^{
        if (initialized) return;
        if (initializeUpdater() && controller.updater.automaticallyChecksForUpdates)
            [controller.updater checkForUpdatesInBackground];
    });
}

void sf_updater_check(void) {
    dispatch_async(dispatch_get_main_queue(), ^{
        BOOL available = initializeUpdater();
        if (!available || delegate.pendingVersion) {
            NSAlert *alert = [NSAlert new];
            alert.messageText = available ? @"Update Ready" : @"Updates Unavailable";
            alert.informativeText = available
                ? [NSString stringWithFormat:@"Petalook %@ has been downloaded. It will be installed when you quit and ready the next time you open the app.", delegate.pendingVersion]
                : startupError;
            [alert addButtonWithTitle:@"Done"];
            [alert runModal];
        } else if (controller.updater.canCheckForUpdates) {
            [controller checkForUpdates:nil];
        } else {
            NSAlert *alert = [NSAlert new];
            alert.messageText = @"Update in Progress";
            alert.informativeText = @"Petalook is already checking for or downloading an update. You can keep viewing your images.";
            [alert addButtonWithTitle:@"Done"];
            [alert runModal];
        }
    });
}
