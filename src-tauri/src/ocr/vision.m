#import <AppKit/AppKit.h>
#import <CoreGraphics/CoreGraphics.h>
#import <Foundation/Foundation.h>
#import <Vision/Vision.h>

typedef void (*shelf_ocr_cb)(void *ctx, double x, double y, double w, double h, const char *text, int vertical);

static NSArray<NSString *> *shelf_default_langs(void) {
    // Chinese first. Putting ja-JP first made Vision read Simplified Chinese as Japanese
    // lookalikes (实力 → 安力) and sprinkle katakana punctuation into otherwise Chinese lines.
    return @[@"zh-Hans", @"zh-Hant", @"ja-JP", @"ko-KR", @"en-US"];
}

static NSArray<NSString *> *shelf_langs_for_text(NSString *text) {
    NSCharacterSet *hira = [NSCharacterSet characterSetWithRange:NSMakeRange(0x3040, 0x60)];
    NSCharacterSet *hangul = [NSCharacterSet characterSetWithRange:NSMakeRange(0xAC00, 0x2BF)];
    if ([text rangeOfCharacterFromSet:hira].location != NSNotFound) {
        return @[@"ja-JP"];
    }
    if ([text rangeOfCharacterFromSet:hangul].location != NSNotFound) {
        return @[@"ko-KR"];
    }
    return @[@"zh-Hans", @"zh-Hant"];
}

static VNRecognizeTextRequest *shelf_request(NSArray<NSString *> *langs, BOOL autoDetect, float minHeight) {
    VNRecognizeTextRequest *req = [[VNRecognizeTextRequest alloc] init];
    req.recognitionLevel = VNRequestTextRecognitionLevelAccurate;
    // Language correction is a Latin spellchecker. On mixed CJK pages it "fixes" 实 into 安.
    req.usesLanguageCorrection = NO;
    req.minimumTextHeight = minHeight;
    req.recognitionLanguages = langs;
    if (@available(macOS 13.0, *)) {
        req.automaticallyDetectsLanguage = autoDetect;
    }
    return req;
}

static NSArray<VNRecognizedTextObservation *> *shelf_run(CGImageRef cg, VNRecognizeTextRequest *req) {
    VNImageRequestHandler *handler = [[VNImageRequestHandler alloc] initWithCGImage:cg options:@{}];
    NSError *err = nil;
    if (![handler performRequests:@[req] error:&err]) {
        return nil;
    }
    return req.results;
}

static VNRecognizedText *shelf_best_candidate(VNRecognizedTextObservation *obs) {
    VNRecognizedText *best = nil;
    for (VNRecognizedText *cand in [obs topCandidates:3]) {
        if (!best || cand.confidence > best.confidence) {
            best = cand;
        }
    }
    return best;
}

static CGImageRef shelf_crop_observation(CGImageRef src, CGRect visionBox) {
    size_t w = CGImageGetWidth(src);
    size_t h = CGImageGetHeight(src);
    if (w == 0 || h == 0) {
        return NULL;
    }
    CGFloat boxW = visionBox.size.width * (CGFloat)w;
    CGFloat boxH = visionBox.size.height * (CGFloat)h;
    CGFloat padX = MAX(4.0, boxW * 0.18);
    CGFloat padY = MAX(4.0, boxH * 0.22);
    CGFloat x = visionBox.origin.x * (CGFloat)w - padX;
    CGFloat y = (1.0 - visionBox.origin.y - visionBox.size.height) * (CGFloat)h - padY;
    CGRect r = CGRectIntersection(
        CGRectMake(x, y, boxW + 2.0 * padX, boxH + 2.0 * padY),
        CGRectMake(0, 0, (CGFloat)w, (CGFloat)h)
    );
    if (r.size.width < 2 || r.size.height < 2) {
        return NULL;
    }
    return CGImageCreateWithImageInRect(src, r);
}

static CGImageRef shelf_upscale_min_height(CGImageRef src, size_t minHeight) {
    size_t w = CGImageGetWidth(src);
    size_t h = CGImageGetHeight(src);
    if (h == 0 || w == 0 || h >= minHeight) {
        CGImageRetain(src);
        return src;
    }
    CGFloat scale = (CGFloat)minHeight / (CGFloat)h;
    size_t nw = MAX((size_t)2, (size_t)ceil((CGFloat)w * scale));
    size_t nh = MAX((size_t)2, (size_t)ceil((CGFloat)h * scale));
    CGColorSpaceRef space = CGColorSpaceCreateDeviceRGB();
    CGContextRef ctx = CGBitmapContextCreate(
        NULL,
        nw,
        nh,
        8,
        0,
        space,
        kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big
    );
    CGColorSpaceRelease(space);
    if (!ctx) {
        CGImageRetain(src);
        return src;
    }
    CGContextSetInterpolationQuality(ctx, kCGInterpolationHigh);
    CGContextDrawImage(ctx, CGRectMake(0, 0, (CGFloat)nw, (CGFloat)nh), src);
    CGImageRef scaled = CGBitmapContextCreateImage(ctx);
    CGContextRelease(ctx);
    if (!scaled) {
        CGImageRetain(src);
        return src;
    }
    return scaled;
}

int shelf_vision_ocr(const uint8_t *data, unsigned long len, const char *lang_csv, shelf_ocr_cb cb, void *ctx) {
    @autoreleasepool {
        NSData *nsdata = [NSData dataWithBytes:data length:len];
        NSImage *image = [[NSImage alloc] initWithData:nsdata];
        if (!image) {
            return -1;
        }

        CGImageRef cg = [image CGImageForProposedRect:NULL context:nil hints:nil];
        if (!cg) {
            return -2;
        }

        NSArray<NSString *> *langs = shelf_default_langs();
        if (lang_csv && strlen(lang_csv) > 0) {
            NSString *s = [NSString stringWithUTF8String:lang_csv];
            NSArray<NSString *> *parsed = [s componentsSeparatedByString:@","];
            if (parsed.count > 0) {
                langs = parsed;
            }
        }

        VNRecognizeTextRequest *pass = shelf_request(langs, YES, 0.01f);
        NSArray<VNRecognizedTextObservation *> *observations = shelf_run(cg, pass);
        if (!observations) {
            return -3;
        }

        for (VNRecognizedTextObservation *obs in observations) {
            VNRecognizedText *best = shelf_best_candidate(obs);
            if (!best) {
                continue;
            }

            NSString *text = best.string ?: @"";
            float confidence = best.confidence;
            if (confidence < 0.28f || text.length == 0) {
                continue;
            }

            BOOL messy = confidence < 0.88f
                || [text hasPrefix:@"."]
                || [text containsString:@"・"]
                || [text containsString:@"･"];
            if (messy) {
                CGImageRef crop = shelf_crop_observation(cg, obs.boundingBox);
                if (crop) {
                    CGImageRef scaled = shelf_upscale_min_height(crop, 72);
                    VNRecognizeTextRequest *refine = shelf_request(shelf_langs_for_text(text), NO, 0.02f);
                    NSArray<VNRecognizedTextObservation *> *refined = shelf_run(scaled, refine);
                    VNRecognizedTextObservation *first = refined.firstObject;
                    VNRecognizedText *better = first ? shelf_best_candidate(first) : nil;
                    if (better && better.confidence >= confidence && better.string.length > 0) {
                        best = better;
                        text = better.string;
                        confidence = better.confidence;
                    }
                    CGImageRelease(scaled);
                    CGImageRelease(crop);
                }
            }

            if (confidence < 0.28f || text.length == 0) {
                continue;
            }

            const char *utf8 = [text UTF8String];
            CGRect r = obs.boundingBox;
            double x = r.origin.x;
            double y = 1.0 - r.origin.y - r.size.height;
            int vertical = (r.size.height > r.size.width * 1.4) ? 1 : 0;
            cb(ctx, x, y, r.size.width, r.size.height, utf8 ? utf8 : "", vertical);
        }
        return 0;
    }
}
