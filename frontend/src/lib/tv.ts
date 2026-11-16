/**
 * TV mode: a fixed, no-scroll layout for the webOS launcher app and the TV's own browser.
 * `?tv=1` forces it on (to preview it on a PC) and `?tv=0` forces it off.
 */
export const isTv: boolean = (() => {
  try {
    const forced = new URLSearchParams(location.search).get('tv');
    if (forced === '1') return true;
    if (forced === '0') return false;
  } catch {
    /* Fall back to user-agent detection. */
  }
  return /Web0S|webOS|SmartTV/i.test(navigator.userAgent);
})();

/**
 * TV mode shows every section open. Apply it once on mount: a reactive `open={isTv}` attribute is
 * written again on every status refresh and would close sections the user just opened.
 */
export const openInTv = (element: HTMLDetailsElement) => {
  if (isTv) element.open = true;
};
