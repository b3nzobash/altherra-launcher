package ua.altherra.client;

import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.networking.v1.ClientPlayConnectionEvents;
import net.fabricmc.fabric.api.client.screen.v1.ScreenEvents;
import net.fabricmc.fabric.api.client.screen.v1.Screens;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.components.AbstractWidget;
import net.minecraft.client.gui.components.Button;
import net.minecraft.client.gui.screens.ConnectScreen;
import net.minecraft.client.gui.screens.DisconnectedScreen;
import net.minecraft.client.gui.screens.PauseScreen;
import net.minecraft.client.gui.screens.Screen;
import net.minecraft.client.gui.screens.TitleScreen;
import net.minecraft.client.multiplayer.ServerData;
import net.minecraft.client.multiplayer.resolver.ServerAddress;
import net.minecraft.network.chat.Component;
import net.minecraft.network.chat.contents.TranslatableContents;

import java.util.List;

/**
 * Altherra Client:
 *  - після кіку чи обриву звʼязку гра сама перепідключається до сервера (з відліком і кнопкою «Скасувати»), скільки б разів це не знадобилося;
 *  - кнопка «Відʼєднатися» в меню паузи стає «Вийти з гри» і закриває Minecraft.
 * Сервер береться з -Daltherra.server=host:port (його передає лаунчер) або з останнього, до якого гравець заходив.
 */
public final class AltherraClient implements ClientModInitializer {
	/** Затримки між спробами, секунди. Далі — щоразу остання. */
	private static final int[] DELAYS = {5, 10, 15, 30};

	private static ServerData lastServer;
	private static int attempt = 0;
	private static boolean cancelled = false;

	@Override
	public void onInitializeClient() {
		String fromLauncher = System.getProperty("altherra.server", "").trim();
		if (!fromLauncher.isEmpty()) {
			lastServer = new ServerData("Altherra", fromLauncher, ServerData.Type.OTHER);
		}

		// Успішно зайшли — запамʼятовуємо сервер і скидаємо лічильник спроб.
		ClientPlayConnectionEvents.JOIN.register((handler, sender, client) -> {
			ServerData current = client.getCurrentServer();
			if (current != null && !client.isLocalServer()) {
				lastServer = current;
			}
			attempt = 0;
			cancelled = false;
		});

		ScreenEvents.AFTER_INIT.register((client, screen, width, height) -> {
			if (screen instanceof DisconnectedScreen) {
				onDisconnected(client, screen, width, height);
			} else if (screen instanceof PauseScreen && !client.isLocalServer() && client.getCurrentServer() != null) {
				replaceDisconnectButton(client, screen);
			}
		});
	}

	/** Екран «Втрачено зʼєднання»: відлік і автоматичне перепідключення. */
	private static void onDisconnected(Minecraft client, Screen screen, int width, int height) {
		if (lastServer == null || cancelled) {
			return;
		}
		int delay = DELAYS[Math.min(attempt, DELAYS.length - 1)];
		long reconnectAt = System.currentTimeMillis() + delay * 1000L;
		List<AbstractWidget> buttons = Screens.getWidgets(screen);

		Button counter = Button.builder(label(delay), b -> {
			cancelled = true;
			b.setMessage(Component.literal("Перепідключення скасовано"));
			b.active = false;
		}).bounds(width / 2 - 100, height - 52, 200, 20).build();
		buttons.add(counter);

		ScreenEvents.afterTick(screen).register(s -> {
			if (cancelled) {
				return;
			}
			long left = reconnectAt - System.currentTimeMillis();
			if (left <= 0) {
				cancelled = true; // щоб не запустити двічі з цього ж екрана
				attempt++;
				ServerData target = lastServer;
				client.execute(() -> {
					cancelled = false;
					ConnectScreen.startConnecting(new TitleScreen(), client, ServerAddress.parseString(target.ip), target, false, null);
				});
			} else {
				counter.setMessage(label((int) Math.ceil(left / 1000.0)));
			}
		});
	}

	private static Component label(int seconds) {
		return Component.literal("Перепідключення через " + seconds + " с — скасувати");
	}

	/** «Відʼєднатися» → «Вийти з гри»: закриває Minecraft. */
	private static void replaceDisconnectButton(Minecraft client, Screen screen) {
		List<AbstractWidget> buttons = Screens.getWidgets(screen);
		for (int i = 0; i < buttons.size(); i++) {
			AbstractWidget w = buttons.get(i);
			if (w instanceof Button old && isDisconnect(old.getMessage())) {
				Button quit = Button.builder(Component.literal("Вийти з гри"), b -> {
					cancelled = true;
					client.stop();
				}).bounds(old.getX(), old.getY(), old.getWidth(), old.getHeight()).build();
				buttons.set(i, quit);
				return;
			}
		}
	}

	private static boolean isDisconnect(Component message) {
		return message.getContents() instanceof TranslatableContents t && t.getKey().equals("menu.disconnect");
	}
}
