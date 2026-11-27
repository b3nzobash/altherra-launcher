package ua.altherra.client;

import io.netty.buffer.Unpooled;
import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.command.v2.ClientCommandRegistrationCallback;
import net.fabricmc.fabric.api.client.command.v2.ClientCommands;
import net.fabricmc.fabric.api.client.networking.v1.ClientLoginNetworking;
import net.fabricmc.fabric.api.client.networking.v1.ClientPlayConnectionEvents;
import net.fabricmc.fabric.api.client.screen.v1.ScreenEvents;
import net.fabricmc.fabric.api.client.screen.v1.Screens;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.components.AbstractWidget;
import net.minecraft.client.gui.components.Button;
import net.minecraft.client.gui.components.StringWidget;
import net.minecraft.client.gui.screens.ConnectScreen;
import net.minecraft.client.gui.screens.DisconnectedScreen;
import net.minecraft.client.gui.screens.PauseScreen;
import net.minecraft.client.gui.screens.Screen;
import net.minecraft.client.gui.screens.TitleScreen;
import net.minecraft.client.multiplayer.ServerData;
import net.minecraft.client.multiplayer.resolver.ServerAddress;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.ChatFormatting;
import net.minecraft.network.chat.Component;
import net.minecraft.network.chat.MutableComponent;
import net.minecraft.network.chat.contents.TranslatableContents;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.concurrent.CompletableFuture;

/**
 * Altherra Client:
 *  - після кіку чи обриву звʼязку гра сама перепідключається до сервера (з відліком, кнопками «Підключитися зараз» і «Скасувати»), скільки б разів це не знадобилося;
 *  - кнопка «Відʼєднатися» в меню паузи стає «Вийти з гри» і закриває Minecraft;
 *  - під час входу відповідає серверу Altherra списком своїх модів (див. {@link ModList});
 *  - зберігає ключ входу від сервера, щоб наступного разу не підтверджувати вхід у Discord (див. {@link LoginKeys}).
 * Сервер береться з -Daltherra.server=host:port (його передає лаунчер) або з останнього, до якого гравець заходив.
 */
public final class AltherraClient implements ClientModInitializer {
	/** Затримки між спробами, секунди. Далі — щоразу остання. */
	private static final int[] DELAYS = {5, 10, 15, 30};

	private static ServerData lastServer;
	private static int attempt = 0;
	private static boolean cancelled = false;
	/** Сервер спитав моди, а у світ ми ще не зайшли: якщо зараз відʼєднає — це відмова через моди, і перепідключення не допоможе. */
	private static boolean modCheckPending = false;
	private static boolean connecting = false;

	@Override
	public void onInitializeClient() {
		String fromLauncher = System.getProperty("altherra.server", "").trim();
		if (!fromLauncher.isEmpty()) {
			lastServer = new ServerData("Altherra", fromLauncher, ServerData.Type.OTHER);
		}

		LoginKeys.register();
		DroneClient.register();
		ClientLoginNetworking.registerGlobalReceiver(ModList.CHANNEL, (client, handler, buf, listenerAdder) -> {
			modCheckPending = true;
			FriendlyByteBuf out = new FriendlyByteBuf(Unpooled.buffer());
			ModList.write(out, handler);
			return CompletableFuture.completedFuture(out);
		});

		// /перезапустити-гру — сервер пропонує її, коли збірку оновлено (посилання в чаті).
		ClientCommandRegistrationCallback.EVENT.register((dispatcher, ctx) -> {
			for (String name : new String[]{"перезапустити-гру", "altherra-relaunch"}) {
				dispatcher.register(ClientCommands.literal(name).executes(c -> {
					relaunch(Minecraft.getInstance());
					return 1;
				}));
			}
		});

		// Успішно зайшли — запамʼятовуємо сервер і скидаємо лічильник спроб.
		ClientPlayConnectionEvents.JOIN.register((handler, sender, client) -> {
			connecting = false;
			ServerData current = client.getCurrentServer();
			if (current != null && !client.isLocalServer()) {
				lastServer = current;
			}
			attempt = 0;
			cancelled = false;
			modCheckPending = false;
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
		connecting = false;
		// Сервер просить підтвердити вхід кодом у Discord — це не про моди: звичайне перепідключення,
		// щоб гра зайшла сама, щойно гравець надішле код боту.
		if (modCheckPending && reasonText(screen).contains("Discord")) {
			modCheckPending = false;
		}
		// Підказку про нові моди показуємо, лише коли сервер справді відмовив через збірку (у таких повідомленнях
		// завжди є «збірк…»). Обрив чи тайм-аут після перевірки модів — наприклад, поки сервер ще завантажується
		// після рестарту, — це звичайне перепідключення.
		if (modCheckPending && !reasonText(screen).toLowerCase(java.util.Locale.ROOT).contains("збірк")) {
			modCheckPending = false;
		}
		if (modCheckPending) {
			// Сервер не пустив через моди: лаунчер має оновити збірку, тож замість перепідключення — підказка.
			modCheckPending = false;
			Button hint = Button.builder(Component.literal(canRelaunch() ? "Перезапустити гру з новими модами" : "Закрий гру й натисни «Грати» в лаунчері"), b -> relaunch(client))
					.bounds(width / 2 - 100, height - 52, 200, 20).build();
			Screens.getWidgets(screen).add(hint);
			return;
		}
		if (lastServer == null) {
			return;
		}
		List<AbstractWidget> widgets = Screens.getWidgets(screen);
		int y = height - 56;

		// Рядок стану над кнопками: відлік або «скасовано».
		StringWidget status = new StringWidget(width / 2 - 150, y, 300, 12, Component.empty(), client.font);
		widgets.add(status);

		// Дві кнопки поруч: підключитися одразу, не чекаючи відліку, і скасувати автопідключення.
		Button now = Button.builder(Component.literal("Підключитися зараз"), b -> {
					b.active = false; // повторні кліки нічого не роблять
					connect(client);
				})
				.bounds(width / 2 - 102, y + 16, 100, 20).build();
		Button cancel = Button.builder(Component.literal("Скасувати"), b -> {
			cancelled = true;
			b.active = false;
		}).bounds(width / 2 + 2, y + 16, 100, 20).build();
		widgets.add(now);
		widgets.add(cancel);

		if (cancelled) {
			// Автопідключення вже скасували раніше — лишаємо лише ручну кнопку.
			status.setMessage(Component.literal("Автопідключення вимкнено").withStyle(ChatFormatting.GRAY));
			cancel.active = false;
			return;
		}
		int delay = DELAYS[Math.min(attempt, DELAYS.length - 1)];
		long reconnectAt = System.currentTimeMillis() + delay * 1000L;
		status.setMessage(statusText(delay));

		ScreenEvents.afterTick(screen).register(s -> {
			if (cancelled) {
				status.setMessage(Component.literal("Автопідключення скасовано").withStyle(ChatFormatting.GRAY));
				return;
			}
			long left = reconnectAt - System.currentTimeMillis();
			if (left <= 0) {
				connect(client);
			} else {
				status.setMessage(statusText((int) Math.ceil(left / 1000.0)));
			}
		});
	}

	/** Підключитися до сервера Altherra зараз (і з відліку, і з кнопки). */
	private static void connect(Minecraft client) {
		ServerData target = lastServer;
		if (target == null || connecting) return;
		// Одне підключення за раз: друге (подвійний клік чи клік разом із відліком) обірвало б перше,
		// а обірване підключення повертає в головне меню. Знімаємо позначку лише на наступному
		// екрані відʼєднання або після входу у світ.
		connecting = true;
		attempt++;
		client.execute(() -> {
			cancelled = false;
			ConnectScreen.startConnecting(new TitleScreen(), client, ServerAddress.parseString(target.ip), target, false, null);
		});
	}

	/** Текст причини відʼєднання з екрана (поле DisconnectionDetails або Component — шукаємо будь-яке). */
	private static String reasonText(Screen screen) {
		StringBuilder out = new StringBuilder();
		for (Class<?> c = screen.getClass(); c != null && c != Object.class; c = c.getSuperclass()) {
			for (var f : c.getDeclaredFields()) {
				try {
					f.setAccessible(true);
					Object v = f.get(screen);
					if (v instanceof Component comp) out.append(comp.getString()).append('\n');
					else if (v instanceof net.minecraft.network.DisconnectionDetails d) out.append(d.reason().getString()).append('\n');
				} catch (Exception ignored) {
				}
			}
		}
		return out.toString();
	}

	private static Component statusText(int seconds) {
		MutableComponent t = Component.literal("Підключимося знову через ").withStyle(ChatFormatting.GRAY)
				.append(Component.literal(seconds + " с").withStyle(ChatFormatting.WHITE));
		if (attempt > 0) t.append(Component.literal("  ·  спроба " + (attempt + 1)).withStyle(ChatFormatting.DARK_GRAY));
		return t;
	}

	private static boolean canRelaunch() {
		return !System.getProperty("altherra.relaunch", "").isBlank();
	}

	/**
	 * Закриває гру й просить лаунчер запустити її знову: лаунчер (він чекає, поки гра працює)
	 * бачить файл-позначку, оновлює збірку й запускає гру. Старий лаунчер без цієї функції — гра просто закривається.
	 */
	static void relaunch(Minecraft client) {
		cancelled = true;
		String flag = System.getProperty("altherra.relaunch", "");
		if (!flag.isBlank()) {
			try {
				Files.writeString(Path.of(flag), "relaunch");
			} catch (Exception ignored) {
			}
		}
		client.execute(client::stop);
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
