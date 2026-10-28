// One JSON value with every canonical-form trap, shared by the Studio's unit test (json.spec.ts)
// and the test against the real server (server/fields.spec.ts), which checks the server stores
// exactly these bytes.

/** A value with every trap at once. The keys are deliberately out of order. */
export const TRICKY = {
	ｱ: null,
	'😀': true,
	é: '\u0001ctrl "quoted" \\ back',
	zeta: 1,
	a: { z: -1.25, y: 9_007_199_254_740_991 },
	Zeta: [1.5, -0, { b: 2, a: 'é' }, [], {}]
};
export const TRICKY_CANONICAL =
	'{"Zeta":[1.5,0,{"a":"é","b":2},[],{}],"a":{"y":9007199254740991,"z":-1.25},"zeta":1,"é":"\\u0001ctrl \\"quoted\\" \\\\ back","😀":true,"ｱ":null}';
