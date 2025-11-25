<script lang="ts" setup>
const route = useRoute();
const pageId = computed(() => `/docs/${route.path}`);
const { data } = await useAsyncData(pageId, () => {
  return queryCollection("docs").path(route.path).first();
});
</script>

<template>
  <div>
    <div v-if="data">
      <h1>{{ data.title }}</h1>
      <ContentRenderer :value="data"> </ContentRenderer>
    </div>
  </div>
</template>
