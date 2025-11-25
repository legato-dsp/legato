<script lang="ts" setup>
const { data: docs } = await useAsyncData(() => {
  return queryCollection("docs").all();
});
</script>

<template>
  <div class="flex flex-col gap-3">
    <h1>Docs</h1>
    <p v-for="doc in docs" :key="doc.id">
      >
      <nuxt-link :to="doc.path">
        <strong>{{ doc.title }}</strong>
      </nuxt-link>

      &nbsp;{{ doc.description }}
    </p>
  </div>
</template>
