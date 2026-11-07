// 1. Remove webdriver flag (primary detection vector)
Object.defineProperty(navigator, 'webdriver', { get: () => undefined });

// 2. Fake chrome runtime object (missing in headless)
window.chrome = {
    runtime: {},
    loadTimes: function() { return {}; },
    csi: function() { return {}; },
};

// 3. Fake plugin array (headless returns empty)
Object.defineProperty(navigator, 'plugins', {
    get: () => {
        const plugins = [
            { name: 'Chrome PDF Plugin', filename: 'internal-pdf-viewer' },
            { name: 'Chrome PDF Viewer', filename: 'mhjfbmdgcfjbbpaeojofohoefgiehjai' },
            { name: 'Native Client', filename: 'internal-nacl-plugin' },
        ];
        plugins.length = 3;
        return plugins;
    }
});

// 4. Realistic language list
Object.defineProperty(navigator, 'languages', {
    get: () => ['en-US', 'en', 'ko-KR', 'ko']
});

// 5. Fix permissions API (headless returns inconsistent states)
const origQuery = window.navigator.permissions.query;
window.navigator.permissions.query = (params) =>
    params.name === 'notifications'
        ? Promise.resolve({ state: Notification.permission })
        : origQuery.call(window.navigator.permissions, params);

// 6. WebGL renderer spoofing (headless exposes "SwiftShader")
const getParam = WebGLRenderingContext.prototype.getParameter;
WebGLRenderingContext.prototype.getParameter = function(param) {
    if (param === 37445) return 'Intel Inc.';
    if (param === 37446) return 'Intel Iris OpenGL Engine';
    return getParam.call(this, param);
};

// 7. Spoof screen + window dimensions (headless defaults to 0x0)
const SCREEN_WIDTH = 1920;
Object.defineProperty(screen, 'width', { get: () => SCREEN_WIDTH });
Object.defineProperty(screen, 'height', { get: () => 1080 });
Object.defineProperty(screen, 'availWidth', { get: () => SCREEN_WIDTH });
Object.defineProperty(screen, 'availHeight', { get: () => 1040 });
Object.defineProperty(screen, 'colorDepth', { get: () => 24 });
Object.defineProperty(window, 'outerWidth', { get: () => SCREEN_WIDTH });
Object.defineProperty(window, 'outerHeight', { get: () => 1080 });

// 8. iframe.contentWindow — headless leaks different prototype chain
const origFrame = HTMLIFrameElement.prototype.__lookupGetter__('contentWindow');
if (origFrame) {
    Object.defineProperty(HTMLIFrameElement.prototype, 'contentWindow', {
        get: function() {
            const win = origFrame.call(this);
            if (win) {
                try { win.chrome = window.chrome; } catch(e) { /* Cross-origin or non-configurable browser properties retain their native behavior. */ }
            }
            return win;
        }
    });
}

// 9. Media codecs — headless Chromium lacks proprietary codecs
const origCanPlay = HTMLMediaElement.prototype.canPlayType;
HTMLMediaElement.prototype.canPlayType = function(type) {
    if (type === 'audio/mp4; codecs="mp4a.40.2"') return 'probably';
    if (type === 'video/mp4; codecs="avc1.42E01E"') return 'probably';
    if (type === 'video/webm; codecs="vp8, vorbis"') return 'probably';
    return origCanPlay.call(this, type);
};

// 10. console.debug trap — CDP leaves debugger traces
const origDebug = console.debug;
console.debug = function() { return undefined; };

// 11. Notification permission (headless returns 'denied' inconsistently)
try {
    Object.defineProperty(Notification, 'permission', { get: () => 'default' });
} catch(e) { /* Cross-origin or non-configurable browser properties retain their native behavior. */ }
