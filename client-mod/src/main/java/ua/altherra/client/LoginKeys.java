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

	/** Ключ для сервера, до якого зараз підключаємося, або "". */
	static String current() {
		String id = id(Minecraft.getInstance());
		return id == null ? "" : load().getOrDefault(id, "");
	}

	/** «адреса|нік» — ключ від одного сервера не надсилаємо іншому. */
	private static String id(Minecraft client) {
		ServerData server = client.getCurrentServer();
		if (server == null || client.isLocalServer()) return null;
		return server.ip.trim().toLowerCase(Locale.ROOT) + "|" + client.getUser().getName().toLowerCase(Locale.ROOT);
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
