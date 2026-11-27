package ua.altherra.client;

import net.fabricmc.fabric.api.creativetab.v1.CreativeModeTabEvents;
import net.minecraft.ChatFormatting;
import net.minecraft.core.component.DataComponents;
import net.minecraft.core.registries.Registries;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.network.chat.Component;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.ResourceKey;
import net.minecraft.world.entity.Display;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.item.CreativeModeTab;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.item.component.CustomData;
import net.minecraft.world.item.component.ItemLore;

import java.util.List;

/**
 * FPV-дрони сервера Altherra на клієнті: 3D-модель (assets/altherra/models/item/fpv_drone*.json),
 * предмети в меню креативу та плавна камера — поворот мишею одразу, без очікування сервера.
 * Сам дрон (політ, батарея, вибух) — на сервері; без сервера Altherra ці предмети нічого не роблять.
 */
final class DroneClient {
	private DroneClient() {
	}

	static void register() {
		ResourceKey<CreativeModeTab> tools = ResourceKey.create(Registries.CREATIVE_MODE_TAB, Identifier.withDefaultNamespace("tools_and_utilities"));
		CreativeModeTabEvents.modifyOutputEvent(tools).register(out -> {
			out.accept(stack(false), CreativeModeTab.TabVisibility.PARENT_AND_SEARCH_TABS);
			out.accept(stack(true), CreativeModeTab.TabVisibility.PARENT_AND_SEARCH_TABS);
		});
	}

	/** Той самий предмет, що дає рецепт на сервері (data/altherra/recipe/fpv_drone*.json). */
	static ItemStack stack(boolean tnt) {
		ItemStack s = new ItemStack(Items.BREEZE_ROD);
		s.set(DataComponents.CUSTOM_NAME, Component.literal(tnt ? "FPV-дрон з ТНТ" : "FPV-дрон").withStyle(s1 -> s1.withItalic(false).withColor(tnt ? ChatFormatting.RED : ChatFormatting.AQUA)));
		s.set(DataComponents.LORE, new ItemLore(tnt
				? List.of(line("ПКМ — запустити, мишею й WASD — керувати", ChatFormatting.GRAY),
						line("Shift — підірвати. Вибухає й від удару", ChatFormatting.GRAY),
						line("Над захищеним містом не руйнує блоків", ChatFormatting.DARK_GRAY))
				: List.of(line("ПКМ — запустити, мишею й WASD — керувати", ChatFormatting.GRAY),
						line("Пробіл — вгору, Ctrl — швидше, Shift — повернутися", ChatFormatting.GRAY),
						line("Літає 90 с і до 64 блоків від тебе", ChatFormatting.DARK_GRAY))));
		s.set(DataComponents.MAX_STACK_SIZE, 1);
		s.set(DataComponents.ENCHANTMENT_GLINT_OVERRIDE, true);
		CompoundTag tag = new CompoundTag();
		tag.putString("altherra", tnt ? "drone_tnt" : "drone");
		s.set(DataComponents.CUSTOM_DATA, CustomData.of(tag));
		s.set(DataComponents.ITEM_MODEL, Identifier.fromNamespaceAndPath("altherra", tnt ? "fpv_drone_tnt" : "fpv_drone"));
		return s;
	}

	private static Component line(String text, ChatFormatting color) {
		return Component.literal(text).withStyle(st -> st.withItalic(false).withColor(color));
	}

	/** Чи це дрон Altherra (сервер показує його як предметний дисплей з моделлю altherra:fpv_drone…). */
	static boolean isDrone(Entity e) {
		if (!(e instanceof Display.ItemDisplay d)) return false;
		Identifier model = d.getItemStack().get(DataComponents.ITEM_MODEL);
		return model != null && model.getNamespace().equals("altherra") && model.getPath().startsWith("fpv_drone");
	}
}
