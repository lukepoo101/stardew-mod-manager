// jsdom for `bun test`, preloaded by bunfig.toml before every test file.
//
// Bun 1.4.4 has no DOM environment of its own (the `[test] environment`
// key in newer docs is silently ignored), so this is a port of what Vitest's
// `environment: "jsdom"` did for this suite: one jsdom window per test file,
// with the same options, and its properties installed on globalThis by the
// same rule (Vitest's `populateGlobal`, vitest/src/integrations/env). Bun
// runs each file in a fresh global (`--parallel` implies `--isolate`), so
// nothing here leaks from one file into another.
import { JSDOM } from "jsdom";

const dom = new JSDOM("<!DOCTYPE html>", {
  url: "http://localhost:3000",
  pretendToBeVisual: true,
  runScripts: "dangerously",
  contentType: "text/html",
});
const win = dom.window as unknown as Record<string, unknown>;
const g = globalThis as unknown as Record<string, unknown>;

// Window properties that replace a same-named runtime global. Every other
// window property is installed only when the runtime has no global of that
// name, so Bun keeps its own fetch, URL, timers, TextEncoder and so on. The
// DOM classes must come from jsdom, or an `Event` built by a test could not
// be dispatched on a jsdom node. This is Vitest's KEYS list.
const OVERRIDDEN = new Set(
  `DOMException EventTarget NamedNodeMap Node Attr Element DocumentFragment
  DOMImplementation Document XMLDocument CharacterData Text CDATASection
  ProcessingInstruction Comment DocumentType NodeList RadioNodeList
  HTMLCollection HTMLOptionsCollection DOMStringMap DOMTokenList StyleSheetList
  HTMLElement HTMLHeadElement HTMLTitleElement HTMLBaseElement HTMLLinkElement
  HTMLMetaElement HTMLStyleElement HTMLBodyElement HTMLHeadingElement
  HTMLParagraphElement HTMLHRElement HTMLPreElement HTMLUListElement
  HTMLOListElement HTMLLIElement HTMLMenuElement HTMLDListElement
  HTMLDivElement HTMLAnchorElement HTMLAreaElement HTMLBRElement
  HTMLButtonElement HTMLCanvasElement HTMLDataElement HTMLDataListElement
  HTMLDetailsElement HTMLDialogElement HTMLDirectoryElement
  HTMLFieldSetElement HTMLFontElement HTMLFormElement HTMLHtmlElement
  HTMLImageElement HTMLInputElement HTMLLabelElement HTMLLegendElement
  HTMLMapElement HTMLMarqueeElement HTMLMediaElement HTMLMeterElement
  HTMLModElement HTMLOptGroupElement HTMLOptionElement HTMLOutputElement
  HTMLPictureElement HTMLProgressElement HTMLQuoteElement HTMLScriptElement
  HTMLSelectElement HTMLSlotElement HTMLSourceElement HTMLSpanElement
  HTMLTableCaptionElement HTMLTableCellElement HTMLTableColElement
  HTMLTableElement HTMLTimeElement HTMLTableRowElement HTMLTableSectionElement
  HTMLTemplateElement HTMLTextAreaElement HTMLUnknownElement HTMLFrameElement
  HTMLFrameSetElement HTMLIFrameElement HTMLEmbedElement HTMLObjectElement
  HTMLParamElement HTMLVideoElement HTMLAudioElement HTMLTrackElement
  HTMLFormControlsCollection SVGElement SVGGraphicsElement SVGSVGElement
  SVGTitleElement SVGAnimatedString SVGNumber SVGStringList Event CloseEvent
  CustomEvent MessageEvent ErrorEvent HashChangeEvent PopStateEvent
  StorageEvent ProgressEvent PageTransitionEvent SubmitEvent UIEvent
  FocusEvent InputEvent MouseEvent KeyboardEvent TouchEvent CompositionEvent
  WheelEvent BarProp External Location History Screen Crypto Performance
  Navigator PluginArray MimeTypeArray Plugin MimeType FileReader FormData Blob
  File FileList ValidityState DOMParser XMLSerializer
  XMLHttpRequestEventTarget XMLHttpRequestUpload XMLHttpRequest WebSocket
  NodeFilter NodeIterator TreeWalker AbstractRange Range StaticRange Selection
  Storage CustomElementRegistry ShadowRoot MutationObserver MutationRecord
  Uint8Array Uint16Array Uint32Array Uint8ClampedArray Int8Array Int16Array
  Int32Array Float32Array Float64Array ArrayBuffer DOMRectReadOnly DOMRect
  Image Audio Option CSS addEventListener alert blur cancelAnimationFrame
  close confirm createPopup dispatchEvent document focus frames
  getComputedStyle history innerHeight innerWidth length localStorage location
  matchMedia moveBy moveTo name navigator open outerHeight outerWidth
  pageXOffset pageYOffset parent postMessage print prompt removeEventListener
  requestAnimationFrame resizeBy resizeTo screen screenLeft screenTop screenX
  screenY scroll scrollBy scrollLeft scrollTo scrollTop scrollX scrollY self
  sessionStorage stop top Window window`.split(/\s+/),
);
const SELF_REFERENCES = ["window", "self", "top", "parent"];

const keys = new Set(
  [...OVERRIDDEN, ...Object.getOwnPropertyNames(win)].filter(
    (key) =>
      !SELF_REFERENCES.includes(key) && (!(key in g) || OVERRIDDEN.has(key)),
  ),
);
const assigned = new Map<string, unknown>();
for (const key of keys) {
  const value = win[key];
  if (typeof value === "function") {
    // Functions and classes never change on the window, so they are plain
    // data properties. Bun's spyOn cannot spy on accessor properties, and
    // tests spy on window.confirm and friends. Plain functions such as
    // getComputedStyle are bound to the window, as Vitest does, so they work
    // when called unqualified. Classes are not.
    Object.defineProperty(g, key, {
      value: key[0] === key[0].toUpperCase() ? value : value.bind(win),
      writable: true,
      configurable: true,
    });
    continue;
  }
  // Everything else (location, scroll position, storage, ...) is read live
  // from the window, and a write reaches the window too.
  Object.defineProperty(g, key, {
    get: () => (assigned.has(key) ? assigned.get(key) : win[key]),
    set: (next: unknown) => {
      assigned.set(key, next);
      win[key] = next;
    },
    configurable: true,
  });
}
for (const key of SELF_REFERENCES) g[key] = g;
Object.defineProperty(dom.window.document, "defaultView", {
  get: () => g,
  enumerable: true,
  configurable: true,
});
g.jsdom = dom;
