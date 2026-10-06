extends RefCounted

static func wait_until(tree: SceneTree, predicate: Callable, timeout_msec: int) -> bool:
	var deadline := Time.get_ticks_msec() + timeout_msec
	while Time.get_ticks_msec() < deadline:
		if predicate.call():
			return true
		await tree.process_frame
	return predicate.call()