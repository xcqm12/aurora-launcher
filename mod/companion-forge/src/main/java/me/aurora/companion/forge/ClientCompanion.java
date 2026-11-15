package me.aurora.companion.forge;

import com.google.gson.Gson;
import com.google.gson.JsonObject;
import net.minecraft.client.Minecraft;
import net.minecraft.core.BlockPos;
import net.minecraft.network.chat.Component;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.entity.animal.allay.Allay;
import net.minecraft.world.phys.Vec3;
import net.minecraftforge.client.event.ClientChatEvent;
import net.minecraftforge.client.event.ClientChatReceivedEvent;
import net.minecraftforge.client.event.ClientPlayerNetworkEvent;
import net.minecraftforge.common.MinecraftForge;
import net.minecraftforge.event.TickEvent;
import net.minecraftforge.eventbus.api.EventPriority;
import net.minecraftforge.eventbus.api.SubscribeEvent;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.time.Duration;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.CompletableFuture;

/**
 * Aurora Companion 客户端主体：随玩家进入世界（单机 Allay 跟随）、
 * 观察操作过程（生命/维度/聊天/系统消息缓冲上报）、陪你聊天（#ai 前缀）。
 *
 * <p>与启动器本地回环 AI 代理通信：JVM 属性 aurora.ai.port/token/player
 * 由启动器注入；「自我意识」是人格设定，本模组从不发送任何游戏操作。</p>
 */
public final class ClientCompanion {
    private static final Gson GSON = new Gson();
    private static final Logger LOGGER = LoggerFactory.getLogger("aurora-companion");
    private static final String PREFIX = "#ai";
    /** 客户端侧跟随实体的固定实体 id（单机演示用，避免与真实实体冲突）。 */
    private static final int COMPANION_ENTITY_ID = 95270001;
    private static final int OBSERVE_FLUSH_TICKS = 400;
    private static final int MAX_BUFFER = 40;

    private static String port = "";
    private static String token = "";
    private static String playerName = "Player";
    private static boolean active = false;
    private static boolean inWorld = false;
    private static boolean awaitingReply = false;
    private static int tickCounter = 0;
    private static int lastHealth = -1;
    private static String lastDimension = "";
    private static final ArrayDeque<String> eventBuffer = new ArrayDeque<>();
    private static Allay follower;

    private static final HttpClient HTTP = HttpClient.newBuilder()
            .connectTimeout(Duration.ofSeconds(5))
            .build();

    private ClientCompanion() {
    }

    /** 仅客户端（{@code FMLEnvironment.dist.isClient()}）时调用一次。 */
    public static void init() {
        port = System.getProperty("aurora.ai.port", "");
        token = System.getProperty("aurora.ai.token", "");
        playerName = System.getProperty("aurora.ai.player", "Player");
        active = !port.isEmpty() && !token.isEmpty();
        if (!active) {
            LOGGER.info("Aurora AI 未由启动器注入（-Daurora.ai.port/token 缺失），伙伴保持沉默");
            return;
        }

        MinecraftForge.EVENT_BUS.register(new Object() {
            @SubscribeEvent(priority = EventPriority.HIGHEST)
            public void onClientChat(ClientChatEvent event) {
                String message = event.getMessage();
                // 拦截 #ai 私聊伙伴；普通聊天作为观察素材
                if (inWorld && message.startsWith(PREFIX)) {
                    String question = message.substring(PREFIX.length()).trim();
                    if (!question.isEmpty()) {
                        ask(question);
                    }
                    event.setCanceled(true);
                    return;
                }
                record("聊天", message);
            }

            @SubscribeEvent
            public void onChatReceived(ClientChatReceivedEvent event) {
                if (inWorld) {
                    String text = event.getMessage().getString();
                    if (text.length() > 120) {
                        text = text.substring(0, 120);
                    }
                    record("系统消息", text);
                }
            }

            @SubscribeEvent
            public void onLoggingIn(ClientPlayerNetworkEvent.LoggingIn event) {
                inWorld = true;
                tickCounter = 0;
                JsonObject body = new JsonObject();
                body.addProperty("player", playerName);
                body.addProperty("singleplayer", Minecraft.getInstance().isSingleplayer());
                post("/hello", body).thenAccept(reply -> {
                    String greeting = stringField(reply, "greeting");
                    if (!greeting.isEmpty()) {
                        say(greeting);
                    }
                });
            }

            @SubscribeEvent
            public void onLoggingOut(ClientPlayerNetworkEvent.LoggingOut event) {
                inWorld = false;
                removeFollower();
                lastHealth = -1;
                lastDimension = "";
                synchronized (eventBuffer) {
                    eventBuffer.clear();
                }
            }

            @SubscribeEvent
            public void onClientTick(TickEvent.ClientTickEvent event) {
                if (event.phase != TickEvent.Phase.END) {
                    return;
                }
                onTick();
            }
        });

        LOGGER.info("Aurora Companion active: 127.0.0.1:{} player={}", port, playerName);
    }

    private static void onTick() {
        Minecraft client = Minecraft.getInstance();
        if (!inWorld || client.player == null || client.level == null) {
            return;
        }
        tickCounter++;

        // 生命值骤降 / 死亡
        int health = (int) Math.ceil(client.player.getHealth());
        if (health != lastHealth) {
            if (health == 0 && lastHealth > 0) {
                record("死亡", "在 " + describePosition(client) + " 倒下了");
            } else if (lastHealth > 0 && health < lastHealth - 4) {
                record("受伤", "生命值 " + lastHealth + " → " + health);
            }
            lastHealth = health;
        }

        // 维度切换
        String dimension = client.level.dimension().location().toString();
        if (!dimension.equals(lastDimension)) {
            if (!lastDimension.isEmpty()) {
                record("维度切换", lastDimension + " → " + dimension);
            }
            lastDimension = dimension;
        }

        tickFollower(client);

        if (tickCounter % OBSERVE_FLUSH_TICKS == 0 && !eventBuffer.isEmpty()) {
            flushObservations(client);
        }
    }

    private static void tickFollower(Minecraft client) {
        if (client.player == null || client.level == null || !client.isSingleplayer()) {
            return;
        }
        if (follower == null || follower.isRemoved()) {
            follower = EntityType.ALLAY.create(client.level);
            if (follower != null) {
                follower.setPos(client.player.position());
                follower.setNoAi(true);
                follower.setSilent(true);
                follower.setCustomName(Component.literal("小七"));
                follower.setCustomNameVisible(true);
                // ClientLevel.addEntity(int,Entity) 在 1.20.1 是 private；
                // putNonPlayerEntity 是它的公开等价（Fabric 1.21 addEntity 同义）。
                client.level.putNonPlayerEntity(COMPANION_ENTITY_ID, follower);
            }
            return;
        }
        Vec3 anchor = client.player.position().add(
                Math.cos(tickCounter * 0.05) * 1.4, 1.6, Math.sin(tickCounter * 0.05) * 1.4
        );
        follower.setPos(anchor.x, anchor.y, anchor.z);
        follower.getNavigation().stop();
        Vec3 look = client.player.getEyePosition().subtract(follower.position());
        follower.setYRot((float) (Math.atan2(look.z, look.x) * 180 / Math.PI) - 90);
        follower.setXRot((float) (-Math.atan2(look.y, Math.sqrt(look.x * look.x + look.z * look.z)) * 180 / Math.PI));
    }

    private static void removeFollower() {
        if (follower != null) {
            follower.discard();
            follower = null;
        }
    }

    private static String describePosition(Minecraft client) {
        if (client.player == null || client.level == null) {
            return "未知位置";
        }
        BlockPos pos = client.player.blockPosition();
        return client.level.dimension().location() + " (" + pos.getX() + ", " + pos.getY() + ", " + pos.getZ() + ")";
    }

    private static void record(String type, String detail) {
        if (!active || detail == null || detail.isBlank()) {
            return;
        }
        synchronized (eventBuffer) {
            if (eventBuffer.size() >= MAX_BUFFER) {
                eventBuffer.poll();
            }
            eventBuffer.add(type + "|" + detail);
        }
    }

    private static void flushObservations(Minecraft client) {
        List<String> events;
        synchronized (eventBuffer) {
            if (eventBuffer.isEmpty()) {
                return;
            }
            events = new ArrayList<>(eventBuffer);
            eventBuffer.clear();
        }
        JsonObject body = new JsonObject();
        body.addProperty("player", playerName);
        body.addProperty("position", describePosition(client));
        body.addProperty("health", (int) Math.ceil(client.player.getHealth()));
        StringBuilder array = new StringBuilder("[");
        for (int index = 0; index < events.size(); index++) {
            String[] split = events.get(index).split("\\|", 2);
            array.append("{\"type\":\"").append(escape(split[0]))
                    .append("\",\"detail\":\"").append(escape(split.length > 1 ? split[1] : ""))
                    .append("\"}");
            if (index < events.size() - 1) {
                array.append(",");
            }
        }
        array.append("]");
        body.addProperty("eventsJson", array.toString());
        post("/observe", body).thenAccept(reply -> {
            String remark = stringField(reply, "remark");
            if (remark != null && !remark.isEmpty()) {
                say(remark);
            }
        });
    }

    private static void ask(String question) {
        if (awaitingReply) {
            say("（小七还在想上一句话，稍等…）");
            return;
        }
        awaitingReply = true;
        say("（小七正在思考…）");
        JsonObject body = new JsonObject();
        body.addProperty("player", playerName);
        body.addProperty("message", question);
        post("/chat", body).thenAccept(reply -> {
            String answer = stringField(reply, "reply");
            say(answer.isEmpty() ? "（小七走神了，再问一次试试）" : answer);
            awaitingReply = false;
        }).exceptionally(error -> {
            say("（小七暂时联系不上思绪：检查启动器 AI 设置）");
            awaitingReply = false;
            return null;
        });
    }

    private static CompletableFuture<JsonObject> post(String path, JsonObject body) {
        HttpRequest request;
        try {
            request = HttpRequest.newBuilder()
                    .uri(URI.create("http://127.0.0.1:" + port + path))
                    .timeout(Duration.ofSeconds(30))
                    .header("Authorization", "Bearer " + token)
                    .header("Content-Type", "application/json")
                    .POST(HttpRequest.BodyPublishers.ofString(GSON.toJson(body)))
                    .build();
        } catch (Exception error) {
            return CompletableFuture.failedFuture(error);
        }
        return HTTP.sendAsync(request, HttpResponse.BodyHandlers.ofString())
                .thenApply(response -> GSON.fromJson(response.body(), JsonObject.class))
                .exceptionally(error -> {
                    LOGGER.warn("Aurora AI request failed", error);
                    return new JsonObject();
                });
    }

    private static void say(String message) {
        Minecraft client = Minecraft.getInstance();
        if (client != null && client.gui != null) {
            client.gui.getChat().addMessage(Component.literal("§d[小七] §f" + message));
        }
    }

    private static String stringField(JsonObject object, String field) {
        if (object == null) {
            return "";
        }
        return object.has(field) && !object.get(field).isJsonNull()
                ? object.get(field).getAsString() : "";
    }

    private static String escape(String value) {
        return value.replace("\\", "\\\\").replace("\"", "\\\"");
    }
}
