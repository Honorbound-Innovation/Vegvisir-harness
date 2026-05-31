import type { BrowserProfile, BrowserProfileName } from "../types.js";

const desktopViewport = { width: 1365, height: 768 };

export const builtInProfiles: Record<BrowserProfileName, BrowserProfile> = {
  "chrome-stable": {
    name: "chrome-stable",
    userAgent:
      "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
    viewport: desktopViewport,
    locale: "en-US",
    timezoneId: "America/New_York",
    colorScheme: "light",
    deviceScaleFactor: 1,
    isMobile: false,
    hasTouch: false,
    extraHTTPHeaders: {
      "Accept-Language": "en-US,en;q=0.9"
    }
  },
  "edge-stable": {
    name: "edge-stable",
    userAgent:
      "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0",
    viewport: desktopViewport,
    locale: "en-US",
    timezoneId: "America/New_York",
    colorScheme: "light",
    deviceScaleFactor: 1,
    isMobile: false,
    hasTouch: false,
    extraHTTPHeaders: {
      "Accept-Language": "en-US,en;q=0.9"
    }
  },
  "firefox-stable": {
    name: "firefox-stable",
    userAgent:
      "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:133.0) Gecko/20100101 Firefox/133.0",
    viewport: desktopViewport,
    locale: "en-US",
    timezoneId: "America/New_York",
    colorScheme: "light",
    deviceScaleFactor: 1,
    isMobile: false,
    hasTouch: false,
    extraHTTPHeaders: {
      "Accept-Language": "en-US,en;q=0.9"
    }
  },
  "safari-desktop": {
    name: "safari-desktop",
    userAgent:
      "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Safari/605.1.15",
    viewport: { width: 1440, height: 900 },
    locale: "en-US",
    timezoneId: "America/Los_Angeles",
    colorScheme: "light",
    deviceScaleFactor: 2,
    isMobile: false,
    hasTouch: false,
    extraHTTPHeaders: {
      "Accept-Language": "en-US,en;q=0.9"
    }
  },
  "generic-desktop": {
    name: "generic-desktop",
    viewport: desktopViewport,
    locale: "en-US",
    timezoneId: "UTC",
    colorScheme: "light",
    deviceScaleFactor: 1,
    isMobile: false,
    hasTouch: false
  }
};

export function resolveProfile(profile?: BrowserProfileName | BrowserProfile): BrowserProfile {
  if (!profile) return builtInProfiles["chrome-stable"];
  if (typeof profile === "string") {
    const builtIn = builtInProfiles[profile];
    if (!builtIn) {
      throw new Error(`Unknown browser profile: ${profile}`);
    }
    return builtIn;
  }
  return profile;
}
