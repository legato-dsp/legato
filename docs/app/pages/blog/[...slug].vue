<script setup lang="ts">
const route = useRoute();

const { data } = await useAsyncData(route.path, () =>
  queryCollection("blog").path(route.path).first()
);
</script>

<template>
  <div class="w-full h-full flex justify-center">
    <div class="w-full max-w-200 h-full overflow-auto pb-32">
      <article v-if="data" class="prose">
        <p class="text-sm">{{ data.date }}</p>
        <h1>{{ data.title }}</h1>
        <ContentRenderer :value="data" />
      </article>
    </div>
  </div>
</template>
