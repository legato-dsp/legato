<script lang="ts" setup>
definePageMeta({ layout: "default" });

const route = useRoute();
const router = useRouter();

const pageId = computed(() => `/docs/${route.path}`);
const { data } = await useAsyncData(pageId, () =>
  queryCollection("docs").path(route.path).first()
);
const { data: docs } = await useAsyncData(() => queryCollection("docs").all());

const currentDoc = computed(() =>
  docs.value?.find((d) => d.path === route.path)
);
const mobileOpen = ref(false);
const dropdownRef = ref<HTMLElement | null>(null);

function navigate(path: string) {
  router.push(path);
  mobileOpen.value = false;
}

function onClickOutside(e: MouseEvent) {
  if (dropdownRef.value && !dropdownRef.value.contains(e.target as Node)) {
    mobileOpen.value = false;
  }
}

onMounted(() => document.addEventListener("mousedown", onClickOutside));
onUnmounted(() => document.removeEventListener("mousedown", onClickOutside));
</script>

<template>
  <div class="w-full h-full">
    <!-- Mobile nav -->
    <div ref="dropdownRef" class="md:hidden w-full relative pt-3 pb-6">
      <button
        @click="mobileOpen = !mobileOpen"
        class="w-full flex items-center justify-between gap-3 px-4 py-2.5 rounded-lg transition-colors"
        style="
          background: #0e0e0f;
          border: 1px solid var(--border);
          color: var(--text-primary);
          font-family: var(--font-display);
          font-size: 0.875rem;
        "
      >
        <span>{{ currentDoc?.title ?? "Select page" }}</span>
        <svg
          :style="{
            transform: mobileOpen ? 'rotate(180deg)' : 'rotate(0deg)',
            transition: 'transform 0.2s',
          }"
          width="16"
          height="16"
          viewBox="0 0 16 16"
          fill="none"
        >
          <path
            d="M4 6l4 4 4-4"
            stroke="var(--text-secondary)"
            stroke-width="1.5"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
      </button>

      <Transition
        enter-active-class="transition-all duration-150 ease-out"
        enter-from-class="opacity-0 -translate-y-1"
        enter-to-class="opacity-100 translate-y-0"
        leave-active-class="transition-all duration-100 ease-in"
        leave-from-class="opacity-100 translate-y-0"
        leave-to-class="opacity-0 -translate-y-1"
      >
        <div
          v-if="mobileOpen"
          class="absolute left-4 right-4 mt-1 rounded-lg overflow-hidden z-50 flex flex-col gap-3"
          style="
            background: #0e0e0f;
            border: 1px solid var(--border);
            top: calc(100% - 0.25rem);
          "
        >
          <NuxtLink
            v-for="doc in docs"
            :key="doc.id"
            :to="doc.path"
            class="w-full text-left px-4 py-2.5 transition-colors"
            style="font-family: var(--font-display); font-size: 0.875rem"
            :style="{
              color:
                doc.path === route.path
                  ? 'var(--text-primary)'
                  : 'var(--text-secondary)',
              background:
                doc.path === route.path
                  ? 'rgba(255,255,255,0.04)'
                  : 'transparent',
            }"
          >
            {{ doc.title }}
          </NuxtLink>
        </div>
      </Transition>
    </div>

    <div class="w-full h-full grid md:grid-cols-[192px_1fr] md:gap-16">
      <!-- Desktop sidebar -->
      <div class="hidden md:flex flex-col gap-3">
        <p
          v-for="doc in docs"
          :key="doc.id"
          :class="doc.path === route.path ? '' : 'opacity-60'"
        >
          <nuxt-link :to="doc.path">{{ doc.title }}</nuxt-link>
        </p>
      </div>

      <div class="w-full h-full flex flex-col items-center overflow-y-auto">
        <div class="h-full w-full max-w-200">
          <div class="w-full">
            <div v-if="data">
              <article class="prose">
                <h1>{{ data.title }}</h1>
                <ContentRenderer :value="data" />
              </article>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
