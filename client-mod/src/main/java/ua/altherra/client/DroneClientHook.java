package ua.altherra.client;

import net.minecraft.world.entity.Entity;

/** Публічний місток для міксина камери (сам DroneClient — у межах пакета). */
public final class DroneClientHook {
	private DroneClientHook() {
	}

	public static boolean isDrone(Entity e) {
		return DroneClient.isDrone(e);
	}
}
