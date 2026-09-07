// A small inline icon set. Everything is drawn with strokes on a 24x24 grid and
// inherits `currentColor`, so a single sprite works in both themes and needs no
// network request (the app runs from file:// inside the webview).
// Exported so a test can check that every name a source file uses is defined
// here. `iconMarkup` throws on an unknown one, at menu-render time, which is a
// bad moment to find out.
export const PATHS = {
  file: '<path d="M13 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z"/><path d="M13 2v7h7"/>',
  folder: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
  save: '<path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z"/><path d="M17 21v-8H7v8"/><path d="M7 3v5h8"/>',
  saveAs:
    '<path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z"/><path d="M12 9v7"/><path d="m9 13 3 3 3-3"/>',
  close: '<path d="M18 6 6 18"/><path d="M6 6l12 12"/>',
  play: '<path d="M8 5v14l11-7z"/>',
  zoomIn:
    '<circle cx="11" cy="11" r="7"/><path d="m20.5 20.5-4.2-4.2"/><path d="M11 8.5v5"/><path d="M8.5 11h5"/>',
  zoomOut: '<circle cx="11" cy="11" r="7"/><path d="m20.5 20.5-4.2-4.2"/><path d="M8.5 11h5"/>',
  zoomReset: '<path d="M3 12a9 9 0 1 0 2.6-6.4"/><path d="M3 3.5V9h5.5"/>',
  sidebar:
    '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16"/><path d="M5.4 8.2h2.2"/><path d="M5.4 11h2.2"/>',
  folderOpen:
    '<path d="M3 8V6a2 2 0 0 1 2-2h4l2 2h6a2 2 0 0 1 2 2v1"/><path d="M3 9h18l-2.1 8.3A2 2 0 0 1 17 19H5a2 2 0 0 1-2-2z"/>',
  toolbar: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 9.5h18"/>',
  statusbar: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 15.5h18"/>',
  sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2v2"/><path d="M12 20v2"/><path d="M2 12h2"/><path d="M20 12h2"/><path d="m4.9 4.9 1.4 1.4"/><path d="m17.7 17.7 1.4 1.4"/><path d="m4.9 19.1 1.4-1.4"/><path d="m17.7 6.3 1.4-1.4"/>',
  moon: '<path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z"/>',
  leaf: '<path d="M11 20A7 7 0 0 1 9.8 6.1C15.5 5 17 4.48 19 2c1 2 2 4.18 2 8 0 5.5-4.78 10-10 10Z"/><path d="M2 21c0-3 1.85-5.36 5.08-6C9.5 14.52 12 13 13 12"/>',
  bold: '<path d="M7 4v16"/><path d="M7 4h6a4 4 0 0 1 0 8H7"/><path d="M7 12h7a4 4 0 0 1 0 8H7"/>',
  help: '<circle cx="12" cy="12" r="9"/><path d="M9.5 9a2.5 2.5 0 0 1 4.9.5c0 1.5-2.4 2-2.4 3.5"/><path d="M12 17h.01"/>',
  warning: '<path d="M12 3 2.5 20h19z"/><path d="M12 10v4"/><path d="M12 17.2h.01"/>',
  chevronDown: '<path d="m6 9 6 6 6-6"/>',
  chevronRight: '<path d="m9 6 6 6-6 6"/>',
  arrowUp: '<path d="M12 19V5"/><path d="m5 12 7-7 7 7"/>',
  arrowDown: '<path d="M12 5v14"/><path d="m19 12-7 7-7-7"/>',
  arrowLeft: '<path d="M19 12H5"/><path d="m12 19-7-7 7-7"/>',
  arrowRight: '<path d="M5 12h14"/><path d="m12 5 7 7-7 7"/>',
  // An arrow up against a wall: the tab goes all the way to the front.
  moveToStart: '<path d="M5 5v14"/><path d="M20 12H9"/><path d="m14 7-5 5 5 5"/>',
  search: '<circle cx="11" cy="11" r="7"/><path d="m20.5 20.5-4.2-4.2"/>',
  cut: '<circle cx="6" cy="6" r="3"/><circle cx="6" cy="18" r="3"/><path d="M20 4 8.12 15.88"/><path d="M14.47 14.48 20 20"/><path d="M8.12 8.12 12 12"/>',
  copy: '<rect x="9" y="9" width="12" height="12" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>',
  paste: '<path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2"/><rect x="8" y="2" width="8" height="4" rx="1"/>',
  selectAll: '<path d="M4 8V6a2 2 0 0 1 2-2h2"/><path d="M16 4h2a2 2 0 0 1 2 2v2"/><path d="M20 16v2a2 2 0 0 1-2 2h-2"/><path d="M8 20H6a2 2 0 0 1-2-2v-2"/><path d="M8 9h8"/><path d="M8 13h6"/>',
  comment: '<path d="M4 5h16"/><path d="M4 12h10"/><path d="M4 19h16"/><path d="M17 10l3 2-3 2"/>',
  exit: '<path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/><path d="M16 17l5-5-5-5"/><path d="M21 12H9"/>',
  // The bar every title-bar minimise button uses, over the window it drops to.
  minimize: '<path d="M6 6h12"/><rect x="4" y="12" width="16" height="8" rx="2"/>',
  upper: '<path d="M4 18 8 6l4 12"/><path d="M5.2 14h5.6"/><path d="M16 8h5"/><path d="M18.5 8v10"/>',
  lower: '<path d="M4 18 7 9l3 9"/><path d="M4.9 15h4.2"/><path d="M20 11a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5z"/><path d="M20 11v5"/>',
  guid: '<rect x="3" y="5" width="18" height="14" rx="2"/><path d="M7 10v4"/><path d="M11 10v4"/><path d="M15 10v4"/><path d="M19 10v4"/>',
  deleteLine: '<path d="M4 7h11"/><path d="M4 12h9"/><path d="M4 17h7"/><path d="m17 10 5 5"/><path d="m22 10-5 5"/>',
  spellcheck: '<path d="m3 15 4.5-10L12 15"/><path d="M4.6 11.5h5.8"/><path d="m14 17 2.5 2.5L22 14"/>',
  bookmark: '<path d="M6 3h12a1 1 0 0 1 1 1v17l-7-4-7 4V4a1 1 0 0 1 1-1z"/>',
  keyboard: '<rect x="2" y="6" width="20" height="12" rx="2"/><path d="M6 10h.01"/><path d="M10 10h.01"/><path d="M14 10h.01"/><path d="M18 10h.01"/><path d="M7 14h10"/>',
  info: '<circle cx="12" cy="12" r="9"/><path d="M12 11v5"/><path d="M12 8h.01"/>',
  globe: '<circle cx="12" cy="12" r="9"/><path d="M3 12h18"/><path d="M12 3a15 15 0 0 1 0 18a15 15 0 0 1 0-18z"/>',
  clock: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
  symbol: '<path d="M9 4H7a3 3 0 0 0-3 3v3a2 2 0 0 1-2 2 2 2 0 0 1 2 2v3a3 3 0 0 0 3 3h2"/><path d="M15 4h2a3 3 0 0 1 3 3v3a2 2 0 0 0 2 2 2 2 0 0 0-2 2v3a3 3 0 0 1-3 3h-2"/>',
  check: '<path d="M20 6 9 17l-5-5"/>',
  link: '<path d="M10 13a5 5 0 0 0 7.1 0l3-3a5 5 0 0 0-7.1-7.1L11.5 4.5"/><path d="M14 11a5 5 0 0 0-7.1 0l-3 3a5 5 0 0 0 7.1 7.1l1.4-1.4"/>',
  terminal: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="m7 9 3 3-3 3"/><path d="M13 15h4"/>',
  terminalAdd: '<path d="M4 6h16"/><path d="m7 11 3 3-3 3"/><path d="M13 17h3"/><path d="M18 9h4"/><path d="M20 7v4"/>',
  shield: '<path d="M12 3 5 6v6c0 4.4 3 8.2 7 9 4-0.8 7-4.6 7-9V6z"/><path d="m9.5 12 2 2 3.5-4"/>',
  sort: '<path d="M4 7h9"/><path d="M4 12h6"/><path d="M4 17h3"/><path d="M17 5v14"/><path d="m14 16 3 3 3-3"/>',
  undo: '<path d="M3 7v6h6"/><path d="M3.5 13a9 9 0 1 0 2.1-6.4L3 9"/>',
  redo: '<path d="M21 7v6h-6"/><path d="M20.5 13a9 9 0 1 1-2.1-6.4L21 9"/>',
  wordWrap: '<path d="M4 6h16"/><path d="M4 12h13a3 3 0 0 1 0 6h-3"/><path d="m16 15-2.5 3 2.5 3"/><path d="M4 18h4"/>',
  // The four splits share a frame; the filled half shows where the pane lands.
  splitUp: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 12h18"/><path d="M6 8h12" opacity=".55"/>',
  splitDown: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 12h18"/><path d="M6 16h12" opacity=".55"/>',
  splitLeft: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M12 4v16"/><path d="M7 8v8" opacity=".55"/>',
  splitRight: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M12 4v16"/><path d="M17 8v8" opacity=".55"/>',
  // The Harness menu and the panel's own tabs. Two lobes and a stem, drawn as a
  // pair of mirrored curves so it reads at 16px rather than turning to mush.
  brain: '<path d="M12 5.5A3 3 0 0 0 6.5 7a3 3 0 0 0-1.4 5.6A3 3 0 0 0 7 18a2.5 2.5 0 0 0 5 .5z"/><path d="M12 5.5A3 3 0 0 1 17.5 7a3 3 0 0 1 1.4 5.6A3 3 0 0 1 17 18a2.5 2.5 0 0 1-5 .5z"/><path d="M12 5.5v13"/>',
  refresh: '<path d="M20 11a8 8 0 1 0-2.3 5.7"/><path d="M20 5v6h-6"/>',
  timeline: '<path d="M5 4v16"/><circle cx="5" cy="8" r="1.6"/><circle cx="5" cy="16" r="1.6"/><path d="M9 8h10"/><path d="M9 16h6"/>',
  chat: '<path d="M20 12a7 7 0 0 1-7 7H9l-4 3v-4.6A7 7 0 0 1 13 5a7 7 0 0 1 7 7z"/>',
  requirements:
    '<path d="M5 4h11l3 3v13a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1z"/><path d="M8 10h8"/><path d="M8 14h8"/><path d="M8 18h5"/>',
  approvals: '<path d="M4 7a2 2 0 0 1 2-2h9l5 5v9a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2z"/><path d="m8.5 13 2.5 2.5 4.5-5"/>',
  diff: '<path d="M6 4v10a3 3 0 0 0 3 3h6"/><path d="m12 14 3 3-3 3"/><circle cx="6" cy="4" r="1.6"/><path d="M18 6v6"/><path d="M15 9h6"/>',
  btw: '<path d="M5 5h14a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2h-7l-4 4v-4H5a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2z"/><path d="M12 8v3"/><path d="M12 13h.01"/>',
  artifacts: '<path d="M12 3l8 4.5v9L12 21l-8-4.5v-9z"/><path d="M12 12l8-4.5"/><path d="M12 12v9"/><path d="m12 12-8-4.5"/>',
};

/** Returns the markup for an icon, sized in `em` so it tracks the button text. */
export function iconMarkup(name, { size = "1em" } = {}) {
  const paths = PATHS[name];
  if (!paths) throw new Error(`Unknown icon: ${name}`);
  const filled = name === "play";
  return (
    `<svg class="icon" viewBox="0 0 24 24" width="${size}" height="${size}" aria-hidden="true" ` +
    `fill="${filled ? "currentColor" : "none"}" stroke="${filled ? "none" : "currentColor"}" ` +
    `stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">${paths}</svg>`
  );
}

/** Builds a detached <svg> element for the same icon. */
export function iconElement(name, options) {
  const holder = document.createElement("span");
  holder.innerHTML = iconMarkup(name, options);
  return holder.firstElementChild;
}

// The application mark, the same artwork the packaged icons are rendered from,
// inlined so the About box needs no request. Regenerate both together with
// `python tools/make-icon.py`.
const APP_LOGO =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAGAAAABgCAYAAADimHc4AAAciElEQVR42u2deZTdVZXvP+f8pjvVvTWkhqQqIxCGEAIRGqVREBUVQeGB7Wwrtth2t9iu1T5oteU9fXZDK8tuh+VSW+U5PiEKtA0CLXNMDCZkkIQMpEKSqlRS47237vCbzjnvj3tvUUSQQCqpEGqvdf9I5davfr/vPnve+7cFL4KMMRaghRCm/u9W4DzgdcCrgEVAO5ACBMcXGaACDAG9wDrgEWCVEGK0jocApBBCHepFxSECLwGEELr+74uA9wNvAnp4ZVMf8N/Aj4UQDzwXXofFAGOM1eCoMeZtwPXA+QedDF2/ljgOT/5zSULjIw963pXAjUKIuw7G7iUxwBhjCyFiY8x84CvAVfX/0s9zA69EmnwAZf1nK4B/EELsbmD4ohnQ4J4x5grg23Xd3uCmxQw9F03GZwj4mBDi9j8lCfIFwL8O+GUd/Lh+4Rnwn58a+MR1zH5pjLmujqV1SAyoi4wyxnwJuLHOVQ3YM/geMtl1zBRwozHmS3VM7T+pgibp/P8J3DTp1IsZTF+yfVB1hlwnhPjXg22CeA618w7gjhnwjwgTLhdC3DnZJohJfqsB5gCbgOY/ZSNm6EVTIx7IA2cA+2phgtANgEU9uv0G0Fr/hRnwp45kHdNW4Bt1rAWAmKR6LgburYvLjKdz5NxUC3izEOI+Y4wlJqmf1cC5Mww4KgxYA7wGELKer3htHXx9rIFvajbqeIoTdB3r1062AVdPCqmPHfBNPbkkxPFmkE0dc6Qxphl4c90oWMfCadfGoIxBCIi05kCxeLxFywJ4szGmWVLLbHbVn3/ajpo2ZuK0SyGwhOBAucJX167igf49E985DkjUse4CzreBCyaJxrRJgBSC2Bj2FcbYnc/zRLHAhmKRPZVh3tmcOx7jAgu4wKZWyWK6I95bN2zgjic2MxwFVF2JaGqiNdtEqRxDpI43BjSwfpVNrYw4LQwwxmCAG+78JT9e9ziZtk68dJJUIoETS0YGhigGPjo2xysDFtnArOlggNYaKSUrVq7iBw//lrY5C5GhQIqA2EjGh8exXYmJI8KqP2V2pqHuJru38uh7WY0/OMumVkA/6t6OlJIwivjBIytxRJKoUEW5As9Y6LKPg43xJAQ+aWUmJIaXAJY2Blk37pMRmOzeNr5zlCllT4vurwM5XCjQu3cQbI9KZRwncihFAoHBER4q1sSlMqd1dzcSVi/JrZVCoIDNIyP8oZBnKAiwJPQkU5ze3MJJ2VxNKur3dRQBEfZ0+fsCKJbLlPPjiKRGuhaxFWMcC8ezUCZgaPQAV194PmcvmP+iT2jj+0II/jA6xoqt23jCrxA4Np60ka5BjI6S2ruPk1NJLpk/n2WtrUddGqa1yuVICzHuE4cgPQfLdbBdl/HRMr7y+cjFr+Vz77oKU48RXkw8IYWgFMXctnkz/3WgD186OIk0riWxpAElwbIpeBZrVcj2bVt4XWs7V51wIk22VbvOUZAGezpdANeycX2FigOkAL9UIqz4nL10Mdd/8GrecNbSF6du6oYd4NE9e/jxli1sCyOSiSRJ4SADjbEVgaVw3QS5dBLHtomDmDD2uWtvP9v3D/HukxezrKvjqEjDtEqAJSWWEKhqmZGBp+npbOXaq9/HX7/zHbhO7daCOCaMY5oSiRdWN1Ly1Ogo3398HY8MjiCSGZLpBEZDIDRIhYwNHdlmMuk0g4ODDPb1UylU0ZbEdmxGtGTLrn289ZSFvHfZUrLJxIS3dCRyUsJMQ6rR1MV7/+Awyy+7Gh2HfPDtb+CTH/tLuusnTxnYNDTIw/uf5qy2Ti7onj/xewfHEVIIylrxo9+vZcUftlKwBKlcMxYSbIFwLAyGVMJjXlcXxo/YvnUHw4UiCeOQsRyUK7CFwBEWSkh0WGVxc5b3nbWEP1+0sHZPWmNJefxIgG1Jrnrj2Xzkve9i2ZKTJ36+tq+fX2zZSL41RYDgVMyzjPezTj2wqnc331izhs3DY6QyaVKuC35AZFtoLVCRobO9jTlt7Qw+PcCu7b1oA6lEAldaGDRoMLaNdG1spcF22T5a5PP3PMQbFvXy0de+hvZ06o/u42UpAQdLQoP68gW+t2Y1d21/gsVnnUFzWzPlcsgV3fN4++x5E6A3fm+wOM73Vq/iV1ufIrQTeKkkCoVlWyAlgYpJpVN0z+6GKKZ3y1ZKY0WSXhrHthCWQNoWWIJEMokjJMXBYUb69zM+mkdog+MIIqWYP2sWH33T6/jLN16IEFPHhGmVAFEHM9aan/x+Lbes38DeSoVzXnUGs9pnU62UkLHATEpFNJhw/7ZtfG/tWtb1H6Ap14ktDFUTYmERxwrLha72NlrSTezv3c3TT+7AQZLKpDE6RutaZ6XtWGRsj0LfAXb37mZsZKRmn1wHx3XQEbhOit1DRa7/3s9Yu3MXN//VB3CsWlb5cM2CPZ25f4whUorP/eed/GrHLlItbSyY08rslnbKFR8tITIKtJ6QGEtKbnnsd/zL/feyYMlSUukm4iAkdkCi0ELT0tJCe2sb1UKRjRvXUBzJk3FSeLaHiA3SMhit8KRE50vs2PkE44MjuAmHWS3NxBiCMMASAttNIhybVFridTSz4rFNLO66l2svfxtaa4SQL1MG1F3G29dv4Bd/2ExH1wLi0CeTdahEPkobHAuUMljiGa9pw8AA3169Cu0lCaoVbCKqYQWBi+em6OhqR8aa7Y9vZHjfAVzLIe0mEEKgRE3qgkqVtONRGBpk365dCK1Jegm0iqj4ZZItWbItaaIoJApjbCmQ0kKpmFxrjh89upp3XXg+nc25P1KjL6VdgunKBQVK8cs/PEkqlSX0y/hxFRybShwRRxFxGCO0wZ1kpW7buB7f9nDiiCAYxxaGuFKluSlDW66JgW27WX//Koaf2kvSWFjCwiiwLJtIR0SVAq9b2EO4axdbHluLKyyS6STG1li2jRCCcrmM7wekm3Lk2lvBFdjC4FrgJW2KAjbunZoikT19xV7BgXyevcNDWNIiDH2MNEgjiKIIqQVaCuI4JtK1ekBfYYz1w/uxmlKE5RF0oDC2TbatGdeP2Lrud/hjJZxEAivholSMZ9loKRgZG+X0rg4+demlXHzGEva86UK+/MOf8ZMHf4u0sjQ1pYmIkZZESvArRaKwTPucubTNns3YWB7LcrA9j6rWjJbLTFXD0PTof8APQ/wwJIojiDUmihFAHMVEYUQURfhBQBjHGGDzwAD9UQXLlRjLQQubTK4JKhW2PLgaf38R13URCHRU6y3LV0qoQoGPnnc2P7n2o1x8xhKMMcyb08XXr/8U/3nTDfzZoh7y4wWkFiQtD096JL0kruNQGCuSyOZonjuHOOGB5yFdh4SbePnHAQ1boKMYo8FYNdbEscLEBksL/DAkVAoBPN4/gDYS4lp7jduUQZRDeldtJBlbyLRLFGmslEesFVF+lFeftIhPX3kprz7t5GcFU8YYjDGcf+ZS7lhyCt+641d8/a57GKuWyLbOQiDwbEkkDKVqiZY5XVRLFQSaVglnzu2ZkujYnu6mHxMpkAatzUQ/t1aaOIyRNlSDAEtajIYB92zbjJdK4Yc+vlbkshn2bdlMUAxItbQQqQhXSErDo2Q8+NQVl/B3V12OJUXdYxETkayoZ0qV1riOwyff+T94y3nncvOtK7hn6w5IZXATCRyhCYMKIopwjGYsX+Rj57+aBe2zpiRPZE/36Y8rPtpNYDAYWatUhzpChSFGWYhSBWN7fPPRh9hXHKYp0UF1vIwlDU1egkLfMK6XIFYK4UiKxVFec0I3N37y4yw9YeGzqm/Pl48y9e+c3N3Ndz71SR7e9ARfv/se1vQPoGyblOcwNjqMKeb5m9eezycufuNhez/HiAQYdBhjiFGmNmZltIYogigmUAo76XLrY79lR/9uvHSasFRmfGCQnhMWQMUnPzBEAg9pNOVqyKtOms+t//JPpJNJlFJYlvW84E/OzlpSTng0F5xxOq8+7RTu37SZtXt3IRIes5ubObdnHkvrxaHjIhdkjCEOQ7SWGKMxQhIrRag1xmgwgqFSnt6xEZxEElsJqqNjSDRzejp5+omtRMVxUimJsC10FHLNZW8hnUwSxTGObb/o1piGxHi2zSXLl3HJ8mV/dM9TWTWbVgYorYmqIVgCjMZYNiqOCYxBEVMeLTBezJNIuNhaEOSLlKtlTlx2GqWwysjAAC4CE8UYS5OwLVozqVoZ8jCylnKSWpqcMhEH1ZWPCy8o8KtIaRBKU1W10F5VfAYHD6DDENd2AEGlkKcyXmL+KSeRaMpyoK8PASgVo0SIZVziMEbVjS2HGSA11NKkxNVxVJCppxKN1hhfYfApBiUWnXkW5fwYB3b3orXATaURUlAeHiO2JAvOXEoim2Z3by9OGGMChTIxysQYPGIdv+wGqqa3HiAlJggoq5ill/w5di5J78aNWK6Nk0oSjuepBAFOron5pyzGdR2e3tkL5Sq2MvjFci1RV8/xGG2QL7PBHjmdfWGOlFSDImdcfB5kbDatfQxLgrBsyqOjVEaGyLVmOGH5EqQFT2/fhqhWEUoRqYgwCJCA0QZ0TY2JGQk49FxQqVRiyXnLSSxoZ91DD9OcTEPSpTgyBhI6Fi+kvWc2+a29FIZHkMaAgUhFNW8pCLEdBy1qNYWaqpYzDDiESgwAieYs8/9sGWs2bsDLpEk1Zdl/YAAch46F82np6mTXpidRI2M4CIIoIgwjjFKYWIExSNchiGK0MBjMjAS8GHKaMgwO9FMeHqWlu5NyvoiUDs0nzaNj/lz6freJyr79GGHwwwihDcK2sP0YHSmUbYEnIFCgBQJZC+RmbMChkR+F7NuzB1uA8UPK4yXclixdC+az78mnGNq1B1mNkdUY27Zxkkkylot/YJQ4jklkElgJCy/hEAUBIga0OQLvpjGY45EBkR9QGMtjGUFlaIwojmnp7iIarzK88SlcYyFch2Qui7RtFrR1MfL0fkyksTEkEg5W0kKkLXAh8Cu4toXS+rABm+gFAgS1yLcxPnUcMKD2ECqKCIMQozRRqYIRkEwmGdm+C1H18ZIe6fZmyrHP37/jcj79tsuoFscRaQ8dKuIwpKV7FlZ7Em92EyoN/SPDWFLWGnJfgjrSk5qwNDAUVOmrlCjqeCISnspGEvtozn49VyAmjEFj0CrGS6SQsaawp49YKDzPYmjfPl536lKuu+xy7r7/QaRl8NIpojDArwY4OqZtURdhNSLRkuZ/3XkbI1GZa956GY5tT6ShXyhz2WgSaAD8+OgQjxWHGIx9fKUJS2W6Q7h08RIWz2o/9rOhk5tkn88LshE4wkFoQ4wh7bjoMCYolWlKpRkfGcWPIs5cfHJtchKIlcTL5vBETKFYRORLRBZ46QSJWSkCv8INd/ycXz++nn+4/EouXLbsBbvaJvL6QvBkMc9DQwPsrBZQCYv8WIG9/f0UyiXiis+ve7fx1Uuu5PTWqakHyCMBvNbPtIav6+1l6+7dz9Krz8Rjoh5EaYysTa7Fce1NLiZS+GFAunUWTjKBFIKWpiwp7TK2f4zWzk7aFvQwXiqR372f/K4B8gOD+OMVvFyWVQf28v5vfpW/+/536RsbnaiC6UlqafLEzHAU8YPdO/ja3idZGecZcwU7e3fz1FNPYSxBpq2ZbE8nYynJv699CF/VVJI5lmzAxCSKFGzp38enf/Qz3v+1b/JEf/9zdxAIgVJqor9GWhYag+PYVKpVvFyOZGuOZMIDYFZzjoztoEs+T29+CkfYLOiZR0I6lIfzVAbyxINloqFxHGWIHJtbHn2UN/7T5/j+vffWdLiUE3+zcXof7u/jn7ds5L7xMYq2pDw2wqbVv2NgXz9WLk2UsHGaM7TO6WDewvkMOLCjMDZhmI8NFWRqJ2lr/z5WrFnLfz2xiZFSQCgdUsn080fEsUIFEVLUJMGyLKTjEOOTbW2DTJqEVZueTSQ8pFObIdYln72PP0m2o4VcexupTBP5kVGKhXFs28EqhUjXJZ1MMFSq8Lff/w53rnyEG973AZafcgoAO8ZGWbHrSTbFFchkUUGVvu07GevbQyadxmrNoTyL9rZZBOMl9u/tpZQvUhrOs7fjJJa2tr/ksakpY0DDeGkh+Nbd93LLo6sZNoq0lyadSqIKw3gHF0bqNx2GEVElwPI0ShpUGCERaAlOIkGquZko6ZG0HAB6uudw8vxuVm7bTrqjFaMVhdE848Vx0rkcbR2dVGOfwmCeuBQj7IgwKOFYHinp8es1m7j/gY/zmQ+8l85zz+TWXVvRHa20pFMM79zKgb17CY0m09yMSSZpau/ANYJtKx+jPDiKsCR4DpVCidAPjhEvyBiMENxw2wr+44FHaG5uIyVddDUkkDFCq1q9ZdKUS0NowyggDgIsKQCNKVUQvo9RCuFK7IRHHIbYtRf1YlsWf/Hm1/Pgut8jerowyifhJTAIKuUyYRTS0tFBZ3cPowcGqZbGkbEmJEZEJZKxIRI2X7rvHtr9fSxYeiopodm+fj2FfQOk21px0ilUJsXchYvwB8fY8OBKgvEKnm1jhEE7FqpQoSvXOiWjpfKwPR0h2LBrF7fcfR9NqSy66hOUqsRVH131qRSq7OobrPnV2jzLGO/q68evVhEGTBwTByEEAcaPUHGEVjHEEbLOslgpPvSeq7jo7OWM7erF8hVRud435Na62oYPDFMcL9DUmqUpl0UrQzxeIqqWsFI2uRO76T7/LLoWzUMVx9nx8CoGt2zHSyaoeoKWnk4Wtnexd/UGVt9+N3G+gpAWQRQhKhHVvcOcms5xxty5tUN1mPMCciqixdXbtlOOFNqPUZWAKAiIqxG6VEFEFrfc+RvCMMK2LaI4njB+P/zNb5Cei45jtDFYjoXyQ3Q1QAQRuhqC0tj1hzRAwkvww5tv5KJlSylWS1gtWSzHxZQCglIFW8eYagU/XyDhWrS05WjunMWsBd3I2c3YPW2kOpupjI+zb8tTlIdGyba3EtuwYE4X9A2yZsWveGL1WlwtUbaFcGycbIa4KYH0HL7woQ+T8rwXNbt2RFRQ44+PVCuUA58mP0RGARqB1pIo9rG1w4btvXzi/3yZL37iGjraa3PhX7v9lzywcxu5XBZT8lFC4XoeYamCjiLQkqhcQaFIed4zA9bG0D27i3tu+Q+++NOf8q2H76UQG9o62/HLPtWRPKYcY1sWYeAgLIERglAoMnM6yS2aTTUOYKxMcWSU5jmzkC1put0UO+5+mNH9Q3iORy6bRTSlaG1vJ+G5lGKfDjfB9R/+OK8/felh152nhgH1k5xMenR3z2ZkzzCtiQRCaUxkMHGMCMpkPYuf3vMIj6xcw5mnLaasI35fHiGZTBOVQ5x6ITydyTA+MlZ7OEDGMaCZ297xrOFqbQyObfOFD36Qv3j9BXzl9lu5c/06VNKidcFsgnKVyK8Sa4WwJF46SXNnO05nG34U4AaSA3v6aJ4/h86e2UR79/PEYw9iGYuE42E5Ntl0klQ2w96du6iOF7ni7HP4/Ds/xEmds/9kn9FRZYA2puYSjo1yyuknsdNy6N+0hSY7gdASozQiighVhGvZ7BrJs3Xlb8mdMJdEcxqn5KOCgKrRZJpbcC2HwfEyDhKjBeXqOEt6elgyb16NAfWHbkiCNobT587nlms/za8fX8vNv/g5K3fuINfaQqq9CWME0rbRnkXVGPRYEa+SpL+vn1xPOyeesJDdj21g99pNtLa1EShFHMU0d3aggoj1v3mU15x8Cp+99tO87dzXvGCT11FnQMMGdHtp1q+4k0uu/jBPtefYvnItpeGRicjTcixSmTTN2Rac1iasjEd+eBQ71pggxEq5tLS2kh8aAaVAWDi2RzWMWL5wESnb+aMHF/V3CjWCu7cuP5s3LDuLf7/9Nv7trjsp+jYtzTm0LUDXpmzGxwscqO6js3s2XdkWtqy4j+E9A7R0tqONQESK9o4OKsUiwf4hvvK31/KJK67Ete1nouYpHtI7rBmxBig7duzkrEuvonnJqZx6zlKasjlKxTJ+qYyIY1QccWBwkMHCKJlcE0YryqMFRFDrgG7umIXblKKQL2JpiPwYK5elqSXDHddfz5mLFr3gyZuc69nUu5ObfvIz7lq/DppSZFJZlI4RKDKZJFLH9K3bio4N6fZW3GwTURigPElrtok3LjiRT73rPSyeO++ITUdO2ZBeI7fy7r/+e2578BGSmSRec5ZEWxuOJYmDgGK1TKQjcu2zSDou40MjaD9ERSHpTBYvlaAwPo4UFlKDMBb5pObat1/GzR+95pDFvtFM1QDr14+u5H9/4zus2b6NXHsrjudArKiM5kFInNYsmWyGoFJltDzOBecs5x/f+wHedMaZNeCVQlrWEe10OWwGNE7HqsfWceEHPkJT+yy0FgRG13S2I7CTCdItWaRl4Y/mUaUKcRyRzGRwHY9SfdjBEjZCQykOOO30hdx307/Sls296DZwbXStiCIEvu9z87e/x5f/7w8ZF4KmXDNRNcQ4AjvhoKKQZKy45qoruf6vPkI2nX5Wku6Il8enYky1cUI/ecMX+drPbqN17kIio7BtcDwb6TrE2hBWAkwQoqOIZNLDdlz8SoSKFbYlsSyLStknm7K567tfZ/lJJx2W0ZusOjZv2843/9/PeWj9JkZKFVzPZmFnO+eefDJXv/MqTj2Cw9hHnAGNYYcojrnmszfw0wcfpaVrNgaNUgoVRbU8jxZopUikEtiOg18NkFGMdFyMtBg7MERHU5oV37iJ889ZPiVgmHp63LJq1ymVK/QPDWHbFvO7urDreSpVz44e7VdkTtmgdqNCFMUx//iVr/K1n9yGcpMkUhmk1thGIeMAy3WxU6la+kALhJFUyz5BPs9Frzqdf/vCZzjt5BOn/CTqep344GsqpSbS1NNBUzopP7lMd8/9D3HjN7/Lmi3b8KMI1/OwXA8nlarN4QYhOohJJDzOmjeXv3n3FbznykuRlnVE1UBNWmtRnZiCQetj7lUFDXUkpUQrzcrfreG223/F1p17GFOaQEU4js2cXI5TFi7k0rdcxPlnnzWhCqY60DnW6Yi9K+K5TnGpXEGpWo9POpU6SBVopBTH22uKD4kB+ki9N25iyMEYpCWZPFeijal1sQmBFBLxytzTYYQxpgSkj9ZI0sGJvFc4lSUwfFCh6oi+HUUIMQP+M1gPS2qLKY8KA2bojxjQK6ltBZ1hwPQwYJ0EHmZmYxLT1JX+sKgvcHiSY2CHwCvo9AtgP3CqFELkqW1PMpOWUc4QR3SRjwHuFULkG6Lw/YPWsc7QkVU/oo45M2uspnuN1aQtep+fweeo0ecb2/RkY9etEOI+aks8G/twZ2hqqbEc9Y6JLXpCqJllnhwDyzzr2/SkEKKf2oIxOWnh2AxNzc55CVxdx7ixwfCZE97Y+CyEuBO4jlrPkDoWmGDMc33My3Gh852NjeU83+KeSVu1vwR8pn6BGRf1pa8stIB/FkJ89uBt2s/b3j5pxe111PbKN4zIzF75Qze4DayuF0LcNHmL9gsy4CAmXAF8G2ifFCnPxAnP7+c38BkCPiaEuP35wP+TXs4km3A7cA6won7hxkpWNWOkn6Xn9SR8VgDn1MG3nw/8Q5qwmcw9Y8zbgOupLQA92MqLSZ/jHfDGRx70vCuBG4UQdx2M3UtmwKQ4oeGuYoy5CHg/8Cag5xUuAX3AfwM/FkI88Fx4HTYDJksDoOthNMaYVuA84HXUloIuqtuK1HEoCQao1HV7L7VC1iPAKiHEaB2P+ssBxCFnlf8/SmK79lykGEkAAAAASUVORK5CYII=";

/**
 * The application mark, the same artwork the packaged icons are rendered from.
 * Inlined rather than loaded from a file so the About box needs no request and
 * stays sharp at any size. Unlike the icons above this one keeps its own
 * colours — it is a logo, not a glyph that should follow the text.
 */
export function appLogoElement(size = 64) {
  const image = document.createElement("img");
  image.className = "app-logo";
  image.width = size;
  image.height = size;
  image.alt = "";
  image.src = APP_LOGO;
  return image;
}
