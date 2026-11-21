/**
 * Latest-request-wins for a page's loads. A page loads the project it opens on, then again whenever the
 * project changes or a write lands, and each load assigns what it gets back; without this, an answer that
 * came back late replaced a newer one (a role created on the chosen project vanished when the opening
 * project's list landed after it, CI 44871512422). Call `start()` as a load begins and check the returned
 * `current()` after every await: it is true only while no later load has started and the page still shows
 * the key (the project) the load was for.
 *
 *   const loads = latestFor(() => projectId);
 *   async function load() {
 *     const current = loads.start();
 *     const reply = await call('GET', url);
 *     if (!current()) return;
 *     items = …;
 *   }
 */
export function latestFor<K>(key: () => K): { start(): () => boolean } {
	let issued = 0;
	return {
		start() {
			const ticket = ++issued;
			const forKey = key();
			return () => ticket === issued && Object.is(forKey, key());
		}
	};
}
