package ua.altherra.client;

import com.google.gson.Gson;
import com.google.gson.reflect.TypeToken;
import net.fabricmc.fabric.api.client.networking.v1.ClientPlayNetworking;
import net.fabricmc.fabric.api.networking.v1.PayloadTypeRegistry;
import net.fabricmc.loader.api.FabricLoader;
import net.minecraft.client.Minecraft;
import net.minecraft.client.multiplayer.ServerData;
import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.codec.ByteBufCodecs;
import net.minecraft.network.codec.StreamCodec;
import net.minecraft.network.protocol.common.custom.CustomPacketPayload;
import net.minecraft.resources.Identifier;

import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.HashMap;
import java.util.Locale;
import java.util.Map;

/**
 * Ключ входу на сервер Altherra: сервер видає його після підтвердження входу через Discord,
 * гра (і лаунчер) зберігає його окремо для кожного сервера й ніка й надсилає наступного разу разом зі списком модів —
 * тоді код у Discord не потрібен. Ключ одноразовий: після кожного входу сервер присилає новий.
 */
final class LoginKeys {
	record KeyPayload(String key) implements CustomPacketPayload {
		static final Type<KeyPayload> TYPE = new Type<>(Identifier.fromNamespaceAndPath("altherra", "login_key"));
		static final StreamCodec<RegistryFriendlyByteBuf, KeyPayload> CODEC = StreamCodec.composite(ByteBufCodecs.STRING_UTF8, KeyPayload::key, KeyPayload::new);

		@Override
		public Type<? extends CustomPacketPayload> type() {
			return TYPE;
		}
	}

	private static final Gson GSON = new Gson();

	static void register() {
		PayloadTypeRegistry.clientboundPlay().register(KeyPayload.TYPE, KeyPayload.CODEC);
		ClientPlayNetworking.registerGlobalReceiver(KeyPayload.TYPE, (payload, ctx) -> {
			String id = id(ctx.client());
			if (id != null && payload.key().length() <= 128) put(id, payload.key());
		});
	}

	/**
	 * Ключ для сервера, до якого зараз підключаємося, або "".
	 * На етапі входу Minecraft.getCurrentServer() ще порожній — адресу беремо з обробника входу (поле serverData).
	 */
	static String current(Object loginHandler) {
		ServerData server = serverOf(loginHandler);
		Minecraft client = Minecraft.getInstance();
		if (server == null) server = client.getCurrentServer();
		if (server == null) return "";
		return load().getOrDefault(normalize(server.ip) + "|" + client.getUser().getName().toLowerCase(Locale.ROOT), "");
	}

	private static ServerData serverOf(Object handler) {
		if (handler == null) return null;
		for (Class<?> c = handler.getClass(); c != null && c != Object.class; c = c.getSuperclass()) {
			for (var f : c.getDeclaredFields()) {
				if (f.getType() != ServerData.class) continue;
				try {
					f.setAccessible(true);
					return (ServerData) f.get(handler);
				} catch (Exception ignored) {
				}
			}
		}
		return null;
	}

	/** «адреса|нік» — ключ від одного сервера не надсилаємо іншому. */
	private static String id(Minecraft client) {
		ServerData server = client.getCurrentServer();
		if (server == null || client.isLocalServer()) return null;
		return normalize(server.ip) + "|" + client.getUser().getName().toLowerCase(Locale.ROOT);
	}

	/** «altherra.com.ua» і «altherra.com.ua:25565» — той самий сервер (лаунчер пише без стандартного порту). */
	static String normalize(String address) {
		String a = address.trim().toLowerCase(Locale.ROOT);
		return a.endsWith(":25565") ? a.substring(0, a.length() - 6) : a;
	}

	private static Path file() {
		return FabricLoader.getInstance().getGameDir().resolve("altherra").resolve("login-keys.json");
	}

	private static synchronized Map<String, String> load() {
		try {
			Map<String, String> m = GSON.fromJson(Files.readString(file()), new TypeToken<HashMap<String, String>>() {}.getType());
			if (m != null) return m;
		} catch (Exception ignored) {
		}
		return new HashMap<>();
	}

	private static synchronized void put(String id, String key) {
		Map<String, String> m = load();
		m.put(id, key);
		try {
			Files.createDirectories(file().getParent());
			Path tmp = file().resolveSibling("login-keys.json.tmp");
			Files.writeString(tmp, GSON.toJson(m));
			Files.move(tmp, file(), StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
		} catch (Exception ignored) {
		}
	}
}
