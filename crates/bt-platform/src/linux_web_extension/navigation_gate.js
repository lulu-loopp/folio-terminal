(() => {
  "use strict";
  const page = globalThis;
  if (page !== page.top) return {installed: true, scope: "subframe"};

  const failure = reason => ({installed: false, reason});
  const navigation = page.navigation;
  const navigateEvent = page.NavigateEvent;
  const navigationDestination = page.NavigationDestination;
  const apply = page.Reflect && page.Reflect.apply;
  const descriptor = page.Object && page.Object.getOwnPropertyDescriptor;
  const create = page.Object && page.Object.create;
  const define = page.Object && page.Object.defineProperty;
  const json = page.JSON;
  const stringify = json && json.stringify;
  const startsWith = page.String && page.String.prototype.startsWith;
  const preventDefault = page.Event && page.Event.prototype.preventDefault;
  const addEventListener = page.EventTarget && page.EventTarget.prototype.addEventListener;
  if (!navigation || !navigateEvent || !navigationDestination) {
    return failure("the browser does not expose the Navigation API");
  }
  if ([apply, descriptor, create, define, stringify, startsWith, preventDefault, addEventListener]
      .some(value => typeof value !== "function")) {
    return failure("the browser is missing an intrinsic required by the navigation policy hook");
  }

  const destination = descriptor(navigateEvent.prototype, "destination");
  const url = descriptor(navigationDestination.prototype, "url");
  const sameDocument = descriptor(navigationDestination.prototype, "sameDocument");
  const cancelable = descriptor(page.Event.prototype, "cancelable");
  if (![destination, url, sameDocument, cancelable].every(value => value && typeof value.get === "function")) {
    return failure("the browser is missing a Navigation API getter required by the policy hook");
  }

  const auth = "__FOLIO_AUTH_TOKEN__";
  const binding = page["__FOLIO_BINDING_NAME__"];
  const permitName = "__FOLIO_PERMIT_NAME__";
  if (typeof binding !== "function") {
    return failure("the private navigation-policy binding is not installed");
  }
  const topFrame = true;
  let sequence = 0;
  let permit = null;

  const allowOnce = (target, proof) => {
    if (proof === auth) permit = target;
  };
  define(page, permitName, {
    value: allowOnce,
    configurable: false,
    enumerable: false,
    writable: false
  });

  const onNavigate = event => {
    if (!topFrame) return;
    const trusted = descriptor(event, "isTrusted");
    if (!trusted || trusted.configurable) return;
    const isTrusted = trusted.get
      ? apply(trusted.get, event, [])
      : trusted.value;
    if (isTrusted !== true) return;
    const target = apply(destination.get, event, []);
    const targetUrl = apply(url.get, target, []);
    if (apply(startsWith, targetUrl, ["http://"]) || apply(startsWith, targetUrl, ["https://"])) return;
    const same = apply(sameDocument.get, target, []);
    if (same && !apply(startsWith, targetUrl, ["javascript:"])) return;
    if (permit === targetUrl) {
      permit = null;
      return;
    }
    const canCancel = apply(cancelable.get, event, []);
    if (canCancel) apply(preventDefault, event, []);
    const payload = create(null);
    payload.token = auth;
    payload.id = ++sequence;
    payload.url = targetUrl;
    payload.cancelable = canCancel;
    apply(binding, undefined, [apply(stringify, json, [payload])]);
  };
  apply(addEventListener, navigation, ["navigate", onNavigate]);
  return {installed: true, scope: "main-frame"};
})();
