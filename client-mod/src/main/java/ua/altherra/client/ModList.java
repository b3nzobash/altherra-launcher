package ua.altherra.client;

import net.fabricmc.loader.api.FabricLoader;
import net.fabricmc.loader.api.ModContainer;
import net.fabricmc.loader.api.metadata.ModOrigin;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.resources.Identifier;

import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HexFormat;
import java.util.List;
import java.util.Map;
import java.util.stream.Stream;

/**
 * Відповідь серверу Altherra на запит altherra:mods: усі jar-файли з теки mods
 * (id мода, версія, назва файлу, SHA-1). Сервер порівнює їх зі збіркою з лаунчера.
 * Рахується один раз за запуск гри: моди під час гри не змінюються.
 */
final class ModList {
	static final Identifier CHANNEL = Identifier.fromNamespaceAndPath("altherra", "mods");
	static final int PROTOCOL = 1;

	private record Entry(String id, String version, String filename, String sha1) {}

	private static List<Entry> cached;

	static void write(FriendlyByteBuf out) {
		List<Entry> mods = list();
		out.writeVarInt(PROTOCOL);
		out.writeVarInt(mods.size());
		for (Entry e : mods) {
			out.writeUtf(cut(e.id(), 128), 128);
			out.writeUtf(cut(e.version(), 128), 128);
			out.writeUtf(cut(e.filename(), 256), 256);
			out.writeUtf(e.sha1(), 64);
		}
		// Відбиток збірки від лаунчера (моди, текстури, шейдери, конфіги). Старий сервер цього поля не читає.
		out.writeUtf(System.getProperty("altherra.build", ""), 64);
		// Ключ входу від сервера (після підтвердження через Discord — без кодів). Старий сервер цього поля не читає.
		out.writeUtf(LoginKeys.current(), 128);
		out.writeBoolean(false); // не «проба»: це справжній вхід у гру
	}

	private static synchronized List<Entry> list() {
		if (cached != null) {
			return cached;
		}
		// Який мод лежить у якому файлі (лише верхній рівень, без вкладених jar).
		Map<Path, ModContainer> byPath = new HashMap<>();
		for (ModContainer c : FabricLoader.getInstance().getAllMods()) {
			if (c.getContainingMod().isPresent() || c.getOrigin().getKind() != ModOrigin.Kind.PATH) {
				continue;
			}
			for (Path p : c.getOrigin().getPaths()) {
				byPath.put(p.toAbsolutePath().normalize(), c);
			}
		}
		List<Entry> out = new ArrayList<>();
		Path dir = FabricLoader.getInstance().getGameDir().resolve("mods");
		try (Stream<Path> files = Files.list(dir)) {
			for (Path f : files.filter(p -> p.getFileName().toString().endsWith(".jar") && Files.isRegularFile(p)).sorted().toList()) {
				ModContainer c = byPath.get(f.toAbsolutePath().normalize());
				String id = c != null ? c.getMetadata().getId() : "";
				String ver = c != null ? c.getMetadata().getVersion().getFriendlyString() : "";
				out.add(new Entry(id, ver, f.getFileName().toString(), sha1(f)));
			}
		} catch (Exception e) {
			// Без списку сервер просто скаже, яких модів бракує.
		}
		cached = List.copyOf(out);
		return cached;
	}

	private static String sha1(Path f) throws Exception {
		MessageDigest md = MessageDigest.getInstance("SHA-1");
		try (InputStream in = Files.newInputStream(f)) {
			byte[] buf = new byte[65536];
			for (int n; (n = in.read(buf)) > 0; ) {
				md.update(buf, 0, n);
			}
		}
		return HexFormat.of().formatHex(md.digest());
	}

	private static String cut(String s, int max) {
		return s.length() <= max ? s : s.substring(0, max);
	}
}
