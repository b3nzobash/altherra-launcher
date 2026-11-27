package ua.altherra.client.mixin;

import net.minecraft.client.Camera;
import net.minecraft.client.Minecraft;
import net.minecraft.world.entity.Entity;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Shadow;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

/**
 * Камера з дрона повертається разом із мишею одразу. Без цього поворот ішов би через сервер
 * (гравець → сервер → дрон → камера) і відставав на пінг.
 */
@Mixin(Camera.class)
abstract class CameraMixin {
	@Shadow
	public abstract Entity entity();

	@Shadow
	protected abstract void setRotation(float yaw, float pitch);

	@Inject(method = "alignWithEntity", at = @At("TAIL"))
	private void altherra$droneLook(float partialTick, CallbackInfo ci) {
		var player = Minecraft.getInstance().player;
		if (player != null && ua.altherra.client.DroneClientHook.isDrone(entity())) {
			setRotation(player.getViewYRot(partialTick), player.getViewXRot(partialTick));
		}
	}
}
