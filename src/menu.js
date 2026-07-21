import { iconElement } from "./icons.js";

/**
 * Renders a list of menu items into `container`.
 *
 * Shared by the menu bar, its submenu flyouts and the editor's context menu, so
 * all three get the same check column, icons, shortcut column and disabled
 * handling. `close` is called before an item runs.
 *
 * @param {HTMLElement} container
 * @param {Array<object>} items
 * @param {() => void} close
 */
export function renderMenuItems(container, items, close) {
  container.textContent = "";
  for (const item of items) {
    if (item.separator) {
      container.append(document.createElement("hr"));
      continue;
    }

    const button = document.createElement("button");
    button.className = "menu-item";
    button.type = "button";

    // A fixed check column keeps every label in a menu aligned, whether or not
    // the item is a toggle.
    const check = document.createElement("span");
    check.className = "menu-check";
    const isChecked = item.checked?.();
    if (isChecked) check.append(iconElement("check"));
    button.append(check);

    if (item.icon) {
      button.append(iconElement(item.icon));
    } else {
      const spacer = document.createElement("span");
      spacer.className = "icon";
      button.append(spacer);
    }

    const label = document.createElement("span");
    label.className = "menu-label";
    label.textContent = item.label;
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

    if (item.enabled && !item.enabled()) button.disabled = true;
    if (isChecked) button.setAttribute("aria-checked", "true");

    if (item.submenu) {
      // The flyout sits beside the button, not inside it — a <button> may not
      // contain other buttons. Its contents are rebuilt on every hover so a
      // list like "recent files" is never stale.
      const wrap = document.createElement("div");
      wrap.className = "menu-item-wrap";
      button.classList.add("has-submenu");

      const flyout = document.createElement("div");
      flyout.className = "menu-flyout";
      flyout.hidden = true;

      const reveal = () => {
        renderMenuItems(flyout, item.submenu(), close);
        flyout.hidden = false;
      };
      if (!button.disabled) {
        wrap.addEventListener("mouseenter", reveal);
        button.addEventListener("click", reveal);
      }
      wrap.addEventListener("mouseleave", () => {
        flyout.hidden = true;
      });

      wrap.append(button, flyout);
      container.append(wrap);
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
  }
}

/**
 * A small click-to-open menu bar.
 *
 * Menus are described as data and re-rendered every time one opens, so items
 * that expose state (the theme in use, whether the toolbar is visible) always
 * show the current value without anything having to invalidate them.
 *
 * @param {HTMLElement} container
 * @param {Array<{label: string, items: Array<object>}>} menus
 */
export function createMenuBar(container, menus) {
  // The bar is rebuilt on every language change. Clearing the container drops
  // the old DOM, but its two document-level listeners would outlive it and keep
  // the whole previous menu tree alive, so the last build is torn down first.
  container.__teardown?.();
  container.textContent = "";
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

  function open(index, byClick) {
    if (openIndex === index) {
      openedByClick = openedByClick || byClick;
      return;
    }
    close();
    openIndex = index;
    openedByClick = byClick;
    titles[index].classList.add("open");
    titles[index].setAttribute("aria-expanded", "true");
    renderMenuItems(dropdowns[index], menus[index].items, close);
    dropdowns[index].hidden = false;
  }

  menus.forEach((menu, index) => {
    const wrapper = document.createElement("div");
    wrapper.className = "menu";

    const title = document.createElement("button");
    title.className = "menu-title";
    title.type = "button";
    title.textContent = menu.label;
    title.setAttribute("aria-haspopup", "true");
    title.setAttribute("aria-expanded", "false");
    title.addEventListener("click", () => {
      if (openIndex === index && openedByClick) close();
      else open(index, true);
    });
    // Once a menu is open, sliding across the bar should follow the pointer.
    title.addEventListener("mouseenter", () => {
      if (openIndex !== -1) open(index, false);
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
    if (event.key === "Escape") close();
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
