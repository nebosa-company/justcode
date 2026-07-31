// What to do about a file that changed underneath an open tab.
//
// The decision and nothing else: no reading, no dialog, no editor. It is here
// rather than inline because the three cases are the whole feature, and the
// version that lived inside the handler could only be checked by arranging for
// a file to change under a running window — which is exactly the situation
// nobody reproduces on purpose.
//
// The editor had no idea a file had moved. The harness watcher has always
// covered `.harness/`, so the panel stayed current while a tab showing one of
// those very files held what it read when it opened, and saving from it would
// have written that back over whatever arrived in between, silently.

/** @typedef {"ignore" | "reload" | "ask"} Verdict */

/**
 * Decide what a changed file means for the tab showing it.
 *
 * - `ignore` — the disk agrees with what the tab last saved, so nothing has
 *   happened that this tab does not already know. The editor's own save lands
 *   here: it moves the file's size and time, which is all the watcher looks at.
 *   Also when the same version was already declined, so saying no once is not
 *   answered by being asked again a second later, forever.
 * - `reload` — nothing has been typed since the last save, so the buffer is a
 *   copy and the file has moved on. Replaced without asking, because there is
 *   nothing to decide and a dialog on every `git checkout` is a dialog people
 *   learn to dismiss without reading.
 * - `ask` — there are edits in the buffer and a different version on disk. Two
 *   versions, and only a person can say which one is wanted.
 *
 * @param {{diskText: string, savedText: string, modified: boolean, declined?: string|null}} state
 * @returns {Verdict}
 */
export function decideReload({ diskText, savedText, modified, declined = null }) {
  if (diskText === savedText) return "ignore";
  if (!modified) return "reload";
  // Declined is compared against the disk text rather than just the path: a
  // *further* change, by something that did not know about the first either, is
  // a new question and not one already answered.
  if (declined !== null && declined === diskText) return "ignore";
  return "ask";
}
