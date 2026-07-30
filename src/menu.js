import { iconElement } from "./icons.js";

/**
 * Renders a list of menu items into `container`, wiring up the keyboard
 * interaction the ARIA "menu" pattern expects — Up/Down to move between
 * items (wrapping at the ends), Home/End to jump, Right to open and step
 * into a submenu, Left/Escape to back out of one. Mouse hover/click keeps
 * working exactly as before; this only adds a second, keyboard-driven way
 * to drive the same menu.
 *
 * Shared by the menu bar, its submenu flyouts and the editor's context menu,
 * so all three get the same check column, icons, shortcut column, disabled
 * handling and now the same keyboard behaviour. `close` is called before an
 * item runs. `parent`, only set for a submenu flyout, is the button that
 * opened it plus the function that hides it — how Left/Escape know what to
 * close and which button to send focus back to.
 *
 * @param {HTMLElement} container
 * @param {Array<object>} items
 * @param {() => void} close
 * @param {{ button: HTMLElement, hide: () => void } | null} parent
 */
export function renderMenuItems(container, items, close, parent = null) {
  container.textContent = "";
  container.setAttribute("role", "menu");
  // Roving tabindex: only ever one stop in the whole menu tree, moved by
  // hand as arrow keys fire, rather than every item sitting in the Tab
  // order — Tab is for moving *past* a menu, arrows are for moving *within*
  // one, per the standard menu keyboard pattern.
  const itemEls = [];
  // One set per menu: the same letter may mean different things in File and in
  // Edit, which is how every menu bar has always worked.
  const claimed = new Set();
  const enabledItems = () => itemEls.filter((el) => !el.disabled);
  const focusEnabledAt = (index) => {
    const list = enabledItems();
    if (!list.length) return;
    list[((index % list.length) + list.length) % list.length].focus();
  };

  for (const item of items) {
    if (item.separator) {
      const hr = document.createElement("hr");
      hr.setAttribute("role", "separator");
      container.append(hr);
      continue;
    }

    const button = document.createElement("button");
    button.className = "menu-item";
    button.type = "button";
    button.tabIndex = -1;

    // A fixed check column keeps every label in a menu aligned, whether or not
    // the item is a toggle.
    const check = document.createElement("span");
    check.className = "menu-check";
    const isChecked = item.checked?.();
    if (item.checked) {
      // A screen reader only reads `aria-checked` as a checkbox state when
      // the role says so — setting the attribute alone on a plain menuitem
      // is silently ignored.
      button.setAttribute("role", "menuitemcheckbox");
      button.setAttribute("aria-checked", String(Boolean(isChecked)));
    } else {
      button.setAttribute("role", "menuitem");
    }
    if (isChecked) check.append(iconElement("check"));
    button.append(check);

    if (item.icon) {
      button.append(iconElement(item.icon));
    } else {
      const spacer = document.createElement("span");
      spacer.className = "icon";
      button.append(spacer);
    }

    // Every item gets a letter, so every item is reachable from the keyboard —
    // which is what "a shortcut for everything" actually needs.
    //
    // Assigned rather than declared: seventeen items had no accelerator, and
    // inventing seventeen global chords would burn muscle-memory space on
    // "Sort Case-Sensitive" and collide with something eventually. A letter
    // within an open menu costs nothing and covers items added later for free.
    //
    // First unused letter of the label, so it is stable while the label is —
    // and skipped entirely once the alphabet in this menu runs out, rather than
    // silently giving two items the same key.
    const letter = claimMnemonic(item.label, claimed);
    const label = document.createElement("span");
    label.className = "menu-label";
    if (letter === -1) {
      label.textContent = item.label;
    } else {
      const mark = document.createElement("u");
      mark.textContent = item.label[letter];
      label.append(
        document.createTextNode(item.label.slice(0, letter)),
        mark,
        document.createTextNode(item.label.slice(letter + 1)),
      );
      button.dataset.mnemonic = item.label[letter].toLowerCase();
    }
    button.append(label);

    const accelerator = document.createElement("span");
    accelerator.className = "menu-accel";
    // A submenu shows a chevron where the shortcut would go.
    accelerator.textContent = item.submenu ? "›" : item.accel || "";
    button.append(accelerator);

    // The accelerator is always visible in the row; the tooltip repeats it on
    // hover so the shortcut is discoverable without reading the whole menu.
    // `hint` (a recent file's full path) is more useful than either.
    button.title = item.hint || (item.accel ? `${item.label} — ${item.accel}` : item.label);

    if (item.enabled && !item.enabled()) {
      button.disabled = true;
      button.setAttribute("aria-disabled", "true");
    }

    if (item.submenu) {
      // The flyout sits beside the button, not inside it — a <button> may not
      // contain other buttons. Its contents are rebuilt on every hover so a
      // list like "recent files" is never stale.
      const wrap = document.createElement("div");
      wrap.className = "menu-item-wrap";
      button.classList.add("has-submenu");
      button.setAttribute("aria-haspopup", "menu");
      button.setAttribute("aria-expanded", "false");

      const flyout = document.createElement("div");
      flyout.className = "menu-flyout";
      flyout.hidden = true;

      const hide = () => {
        flyout.hidden = true;
        button.setAttribute("aria-expanded", "false");
      };
      const reveal = (focusFirst) => {
        renderMenuItems(flyout, item.submenu(), close, { button, hide });
        flyout.hidden = false;
        button.setAttribute("aria-expanded", "true");
        place(button, flyout);
        if (focusFirst) {
          const first = flyout.querySelector(".menu-item:not(:disabled)");
          first?.focus();
        }
      };
      if (!button.disabled) {
        wrap.addEventListener("mouseenter", () => reveal(false));
        button.addEventListener("click", () => reveal(false));
        button.addEventListener("keydown", (event) => {
          if (event.key === "ArrowRight") {
            event.preventDefault();
            event.stopPropagation();
            reveal(true);
          }
        });
      }
      wrap.addEventListener("mouseleave", hide);

      wrap.append(button, flyout);
      container.append(wrap);
      itemEls.push(button);
      continue;
    }

    button.addEventListener("click", () => {
      close();
      // Menu actions are frequently async; without this a rejection is an
      // unhandled promise and the user sees nothing at all happen.
      try {
        const result = item.run();
        if (result && typeof result.catch === "function") {
          result.catch((error) => console.error("Menu action failed:", error));
        }
      } catch (error) {
        console.error("Menu action failed:", error);
      }
    });
    container.append(button);
    itemEls.push(button);
  }

  container.addEventListener("keydown", (event) => {
    const list = enabledItems();
    if (!list.length) return;
    // -1 (nothing in this menu focused yet) is a deliberately usable index
    // below: Up wraps to the last item, Down lands on the first.
    const index = list.indexOf(document.activeElement);

    if (event.key === "ArrowDown") {
      event.preventDefault();
      event.stopPropagation();
      focusEnabledAt(index + 1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      event.stopPropagation();
      focusEnabledAt(index - 1);
    } else if (event.key === "Home") {
      event.preventDefault();
      event.stopPropagation();
      focusEnabledAt(0);
    } else if (event.key === "End") {
      event.preventDefault();
      event.stopPropagation();
      focusEnabledAt(-1);
    } else if (parent && event.key === "ArrowLeft") {
      // Backs out of just this submenu. Escape deliberately is not handled
      // here too: the menu bar's own Escape listener runs in the capture
      // phase, so it always sees the key first regardless of anything this
      // bubble-phase handler does, and closes the whole menu tree — which
      // is a fine, simple answer for Escape (close everything) as long as
      // ArrowLeft is the one that steps back exactly one level.
      event.preventDefault();
      event.stopPropagation();
      parent.hide();
      parent.button.focus();
    }
  });
}

/**
 * A small click-to-open menu bar.
 *
 * Menus are described as data and re-rendered every time one opens, so items
 * that expose state (the theme in use, whether the toolbar is visible) always
 * show the current value without anything having to invalidate them.
 *
 * `mnemonic`, a single letter, gives a top-level menu an Alt+<letter> shortcut
 * that opens it from anywhere — the underlined-letter convention every
 * Windows menu bar uses. It is matched against `event.key`, not the rendered
 * label, so it keeps working under a translation whose label doesn't
 * literally contain that letter (the underline just won't show in that case).
 *
 * @param {HTMLElement} container
 * @param {Array<{label: string, mnemonic?: string, items: Array<object>}>} menus
 */
export function createMenuBar(container, menus) {
  // The bar is rebuilt on every language change. Clearing the container drops
  // the old DOM, but its two document-level listeners would outlive it and keep
  // the whole previous menu tree alive, so the last build is torn down first.
  container.__teardown?.();
  container.textContent = "";
  container.setAttribute("role", "menubar");
  let openIndex = -1;
  const titles = [];
  const dropdowns = [];

  function close() {
    if (openIndex === -1) return;
    titles[openIndex].classList.remove("open");
    titles[openIndex].setAttribute("aria-expanded", "false");
    dropdowns[openIndex].hidden = true;
    openIndex = -1;
  }

  // Tracks whether the open menu was opened by a click rather than a hover, so
  // a click on a title that hover already opened keeps it open instead of
  // immediately toggling it shut.
  let openedByClick = false;

  function open(index, byClick, focusFirst = false) {
    if (openIndex === index) {
      openedByClick = openedByClick || byClick;
      if (focusFirst) dropdowns[index].querySelector(".menu-item:not(:disabled)")?.focus();
      return;
    }
    close();
    openIndex = index;
    openedByClick = byClick;
    titles[index].classList.add("open");
    titles[index].setAttribute("aria-expanded", "true");
    renderMenuItems(dropdowns[index], menus[index].items, close);
    dropdowns[index].hidden = false;
    if (focusFirst) dropdowns[index].querySelector(".menu-item:not(:disabled)")?.focus();
  }

  menus.forEach((menu, index) => {
    const wrapper = document.createElement("div");
    wrapper.className = "menu";

    const title = document.createElement("button");
    title.className = "menu-title";
    title.type = "button";
    const mnemonicIndex = menu.mnemonic
      ? menu.label.toLowerCase().indexOf(menu.mnemonic.toLowerCase())
      : -1;
    if (mnemonicIndex === -1) {
      title.textContent = menu.label;
    } else {
      const mark = document.createElement("u");
      mark.textContent = menu.label[mnemonicIndex];
      title.append(
        document.createTextNode(menu.label.slice(0, mnemonicIndex)),
        mark,
        document.createTextNode(menu.label.slice(mnemonicIndex + 1)),
      );
    }
    title.setAttribute("role", "menuitem");
    title.setAttribute("aria-haspopup", "menu");
    title.setAttribute("aria-expanded", "false");
    if (menu.mnemonic) title.setAttribute("aria-keyshortcuts", `Alt+${menu.mnemonic.toUpperCase()}`);
    title.addEventListener("click", () => {
      if (openIndex === index && openedByClick) close();
      else open(index, true);
    });
    // Once a menu is open, sliding across the bar should follow the pointer.
    title.addEventListener("mouseenter", () => {
      if (openIndex !== -1) open(index, false);
    });
    title.addEventListener("keydown", (event) => {
      // Left/Right walk the bar itself — opening whichever title's menu was
      // already showing one, so arrowing across File ▸ Edit ▸ View behaves
      // like sliding the mouse across them.
      if (event.key === "ArrowRight" || event.key === "ArrowLeft") {
        event.preventDefault();
        const delta = event.key === "ArrowRight" ? 1 : -1;
        const next = (index + delta + titles.length) % titles.length;
        titles[next].focus();
        if (openIndex !== -1) open(next, openedByClick);
      } else if (event.key === "ArrowDown" || event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        open(index, true, true);
      }
    });

    const dropdown = document.createElement("div");
    dropdown.className = "menu-dropdown";
    dropdown.hidden = true;

    wrapper.append(title, dropdown);
    container.append(wrapper);
    titles.push(title);
    dropdowns.push(dropdown);
  });

  // Capture phase, so a click anywhere outside dismisses the menu even over a
  // widget that stops the event before it can bubble up to the document — the
  // editor and the find bar both do.
  const onDocumentMouseDown = (event) => {
    if (!container.contains(event.target)) close();
  };
  const onDocumentKeyDown = (event) => {
    if (event.key === "Escape" && openIndex !== -1) {
      const focused = titles[openIndex];
      close();
      focused.focus();
      return;
    }
    // A bare letter activates the item in the open menu that claimed it.
    //
    // Here rather than on the dropdown, because clicking a menu title leaves
    // focus on the title — a sibling of the dropdown — so a listener on the
    // container never sees the keypress. This handler is the only one that sees
    // it either way.
    //
    // Plain keypress only: Ctrl and Alt belong to the accelerators, and a letter
    // typed with no menu open belongs to the editor.
    if (
      openIndex !== -1 &&
      event.key.length === 1 &&
      !event.ctrlKey &&
      !event.altKey &&
      !event.metaKey &&
      /[a-z0-9]/i.test(event.key)
    ) {
      const wanted = event.key.toLowerCase();
      const items = [...dropdowns[openIndex].querySelectorAll(".menu-item")];
      const match = items.find(
        (element) => !element.disabled && element.dataset.mnemonic === wanted,
      );
      if (match) {
        event.preventDefault();
        event.stopPropagation();
        match.click();
        return;
      }
    }

    // Alt+<mnemonic> opens the matching top-level menu from anywhere, not
    // just while the bar already has focus — mirrors Windows' menu-bar
    // access keys. Plain Alt only: Ctrl+Alt is AltGr on many layouts, and
    // Alt+Shift+letter is a layout-switch chord on some systems.
    if (event.altKey && !event.ctrlKey && !event.shiftKey && event.key.length === 1) {
      const key = event.key.toLowerCase();
      const index = menus.findIndex((menu) => menu.mnemonic === key);
      if (index !== -1) {
        event.preventDefault();
        open(index, true, true);
        titles[index].focus();
      }
    }
  };
  document.addEventListener("mousedown", onDocumentMouseDown, true);
  document.addEventListener("keydown", onDocumentKeyDown, true);
  // Clicking into another window, or a native dialog opening, should not leave
  // a menu hanging open on top of everything.
  window.addEventListener("blur", close);

  container.__teardown = () => {
    document.removeEventListener("mousedown", onDocumentMouseDown, true);
    document.removeEventListener("keydown", onDocumentKeyDown, true);
    window.removeEventListener("blur", close);
    delete container.__teardown;
  };

  return { close, destroy: container.__teardown };
}

let openContextMenu = null;
// Removes the dismiss listeners belonging to the menu currently on screen.
let releaseContextMenu = null;

/** Dismisses the editor context menu, if one is showing. */
export function closeContextMenu() {
  if (!openContextMenu) return;
  openContextMenu.remove();
  openContextMenu = null;
  // Choosing an item closes the menu through here, so the listeners have to be
  // released here too — otherwise they outlive their menu and the next menu is
  // torn down mid-click by the previous one's handler.
  const release = releaseContextMenu;
  releaseContextMenu = null;
  release?.();
}

/**
 * Shows a context menu at viewport coordinates `x`/`y`, nudged back inside the
 * window if it would otherwise hang off the bottom or the side.
 */
/** The index of the letter this item should underline, or -1 for none.
 *
 * First character not already claimed in this menu, preferring the start of a
 * word — `Save As…` takes `S` then `A` rather than `S` then `v`, which is what
 * anyone scanning the menu would guess.
 *
 * Returns -1 rather than doubling up when everything is taken: two items
 * answering the same key is worse than one item having no key.
 */
function claimMnemonic(label, claimed) {
  const text = String(label ?? "");
  const starts = [];
  const rest = [];
  for (let index = 0; index < text.length; index += 1) {
    if (!/[a-z0-9]/i.test(text[index])) continue;
    const atWordStart = index === 0 || !/[a-z0-9]/i.test(text[index - 1]);
    (atWordStart ? starts : rest).push(index);
  }
  for (const index of [...starts, ...rest]) {
    const key = text[index].toLowerCase();
    if (claimed.has(key)) continue;
    claimed.add(key);
    return index;
  }
  return -1;
}

/** Put a submenu flyout beside its item, in viewport coordinates.
 *
 * The flyout is `position: fixed` rather than absolute, because a dropdown tall
 * enough to need `overflow-y: auto` clips absolutely-positioned descendants —
 * and a non-visible overflow on one axis makes the other compute to `auto`, so
 * "Recent" turned into a horizontal scrollbar inside the parent instead of a
 * list to the right of it. Fixed escapes the clip; staying a DOM child of the
 * item's wrapper keeps `mouseleave` working, which moving it to `<body>` would
 * have broken.
 */
function place(button, flyout) {
  const item = button.getBoundingClientRect();
  const { width, height } = flyout.getBoundingClientRect();
  const rtl = getComputedStyle(button).direction === "rtl";

  // Beside the item, flipped to its other side when there is no room — a menu
  // near the right edge must not run off it.
  let left = rtl ? item.left - width : item.right;
  if (!rtl && left + width > window.innerWidth - 4) left = item.left - width;
  if (rtl && left < 4) left = item.right;
  left = Math.max(4, Math.min(left, window.innerWidth - width - 4));

  // Aligned with the item, lifted just enough to stay on screen.
  const top = Math.max(4, Math.min(item.top - 4, window.innerHeight - height - 4));

  flyout.style.insetInlineStart = "auto";
  flyout.style.insetInlineEnd = "auto";
  flyout.style.left = `${left}px`;
  flyout.style.top = `${top}px`;
}

export function showContextMenu(x, y, items) {
  closeContextMenu();

  const menu = document.createElement("div");
  menu.className = "menu-dropdown context-menu";
  renderMenuItems(menu, items, closeContextMenu);
  document.body.append(menu);
  openContextMenu = menu;

  // Measured after insertion — the height depends on how many items there are.
  const { width, height } = menu.getBoundingClientRect();
  const left = Math.max(4, Math.min(x, window.innerWidth - width - 4));
  const top = Math.max(4, Math.min(y, window.innerHeight - height - 4));
  // The stylesheet pins the start edge logically, which resolves to `right` in
  // an RTL interface. Left as-is, both edges end up set on an auto-width fixed
  // box and it stretches across the window, so clear them before positioning.
  menu.style.insetInlineStart = "auto";
  menu.style.insetInlineEnd = "auto";
  menu.style.left = `${left}px`;
  menu.style.top = `${top}px`;
  // A right-click opens this with the mouse, but a keyboard user reaching it
  // via the Menu key or Shift+F10 needs the same starting focus a dropdown
  // gets, rather than being left on whatever was focused in the editor.
  menu.querySelector(".menu-item:not(:disabled)")?.focus();

  const dismiss = (event) => {
    if (event.type === "mousedown" && menu.contains(event.target)) return;
    if (event.type === "keydown" && event.key !== "Escape") return;
    closeContextMenu();
  };
  releaseContextMenu = () => {
    document.removeEventListener("mousedown", dismiss, true);
    document.removeEventListener("keydown", dismiss, true);
    window.removeEventListener("blur", dismiss);
  };
  document.addEventListener("mousedown", dismiss, true);
  document.addEventListener("keydown", dismiss, true);
  window.addEventListener("blur", dismiss);
}
